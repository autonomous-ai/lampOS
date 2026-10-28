//! What an update touches besides the filesystem, each behind a trait so
//! the tests play it: systemd, the bus (what the voice says and the lamp's
//! health), the disk's free room and the clock. The real ones are here.
use crate::{judge::UnitState, quiet::Heard};
use lamp_bus::{Bus, Subscriber};
use lamp_msgs::{Health, Speech, VoiceMute, VoiceState};
use std::{
    collections::BTreeMap,
    io,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub(crate) mod query;

pub trait Systemd {
    /// Each unit's state, as `systemctl show` gives it.
    fn states(&mut self, units: &[String]) -> Result<Vec<UnitState>, String>;
    /// Fresh effective product Setup identity after reload, including loaded
    /// aliases and drop-ins. Unknown managers retain normal restart/watch.
    fn setup_unit_properties(&mut self) -> Option<String> {
        None
    }
    /// The installed fallback can run, and its deadline timer is armed now
    /// and enabled persistently. Reads systemd's state; changes nothing.
    fn self_recovery_ready(&mut self) -> Result<(), String>;
    /// `systemctl daemon-reload`: unit files written are read again.
    fn reload(&mut self) -> Result<(), String>;
    /// `systemctl restart`: returns once systemd has done it.
    fn restart(&mut self, unit: &str) -> Result<(), String>;
    /// `systemctl stop`, of a unit a round added and takes away again
    /// (crate::added): one systemd never loaded is stopped already.
    /// Refused unless a world can.
    fn stop(&mut self, unit: &str) -> Result<(), String> {
        Err(format!("{unit} cannot be stopped here"))
    }
    /// `systemctl show` of [`crate::writer_unit::PROPERTIES`] for the
    /// settings writer's one-shot, [`crate::writer_unit::UNIT`], and only
    /// it. A systemd that cannot say refuses: the unit is then never started.
    fn writer_unit_properties(&mut self) -> Result<String, String> {
        Err("this systemd cannot show the writer's unit".to_owned())
    }
    /// `systemctl start` of [`crate::writer_unit::UNIT`], and only it:
    /// returns once the one-shot has ended. A systemd that cannot refuses.
    fn start_writer_unit(&mut self) -> Result<(), String> {
        Err("this systemd cannot start the writer's unit".to_owned())
    }
    /// The last line the unit's own program wrote to the journal since this
    /// program last restarted it, cleaned ([`clean_line`]); none before a
    /// restart, or when it said nothing since (#1505). Only ever asked of a
    /// unit a round has found not well.
    fn last_line(&mut self, unit: &str) -> Option<String>;
}

pub trait Ears {
    /// What the voice said on the bus in the next `wait`, which this takes.
    fn listen(&mut self, wait: Duration) -> Vec<Heard>;
    /// The lamp's health, if it is published and has not expired.
    fn health(&mut self) -> Option<Health>;
}

pub trait Disk {
    /// The bytes an ordinary program may still write on the disk `path` is on.
    fn free(&self, path: &Path) -> io::Result<u64>;
}

pub trait Clock {
    /// Milliseconds on a clock that only goes forward.
    fn now_ms(&self) -> u64;
}

/// The real systemd.
pub struct Systemctl {
    /// The longest a restart may take: the body folds down before it stops,
    /// up to 25 s.
    pub restart_within: Duration,
    /// When each unit was last restarted here, on the wall clock the
    /// journal keeps, and its main process just before: the lines since,
    /// but that process's own last words as it stopped, are the new build's.
    restarted: Vec<(String, SystemTime, u32)>,
}

impl Default for Systemctl {
    fn default() -> Self {
        Self {
            restart_within: RESTART_WITHIN,
            restarted: Vec::new(),
        }
    }
}

impl Systemd for Systemctl {
    fn states(&mut self, units: &[String]) -> Result<Vec<UnitState>, String> {
        let stdout = show_within(
            Command::new("systemctl")
                .args([
                    "show",
                    "--property=Id,ActiveState,SubState,MainPID,NRestarts",
                    "--",
                ])
                .args(units),
            JOURNAL_WITHIN,
        )?;
        Ok(parse_show(&stdout))
    }

    fn setup_unit_properties(&mut self) -> Option<String> {
        output_within(
            Command::new("systemctl").args([
                "show",
                "--no-pager",
                "--property=Id,Names,LoadState,FragmentPath,DropInPaths",
                "--",
                "lamp-setup.service",
            ]),
            JOURNAL_WITHIN,
        )
    }

    fn self_recovery_ready(&mut self) -> Result<(), String> {
        let text = output_within(
            Command::new("systemctl").args([
                "show",
                "--no-pager",
                "--property=Id,LoadState,UnitFileState,ActiveState,SubState,Unit",
                "--",
                SELF_BACK_SERVICE,
                SELF_BACK_TIMER,
            ]),
            JOURNAL_WITHIN,
        )
        .ok_or("self recovery: the systemctl query failed or timed out")?;
        self_recovery_show(&text)
    }

    fn reload(&mut self) -> Result<(), String> {
        run(&["daemon-reload"], self.restart_within)
    }

    fn writer_unit_properties(&mut self) -> Result<String, String> {
        use crate::writer_unit::{PROPERTIES, UNIT};
        show_within(
            Command::new("systemctl").args([
                "show",
                "--no-pager",
                &format!("--property={PROPERTIES}"),
                "--",
                UNIT,
            ]),
            JOURNAL_WITHIN,
        )
    }

    fn start_writer_unit(&mut self) -> Result<(), String> {
        // Beyond the unit's own TimeoutStartSec (120 s), so a wait that ends
        // means the unit did (Seventh's P3 on #4107).
        run(&["start", crate::writer_unit::UNIT], WRITER_WITHIN)
    }

    fn restart(&mut self, unit: &str) -> Result<(), String> {
        // Asked within the journal's bound, so a slow systemd delays the
        // restart by that at most; not known, nothing is skipped.
        let before = output_within(
            Command::new("systemctl").args(["show", "--property=MainPID", "--value", "--", unit]),
            JOURNAL_WITHIN,
        )
        .and_then(|pid| pid.trim().parse().ok())
        .unwrap_or(0);
        let at = SystemTime::now();
        self.restarted.retain(|(name, _, _)| name != unit);
        self.restarted.push((unit.to_owned(), at, before));
        run(&["restart", unit], self.restart_within)
    }

    fn stop(&mut self, unit: &str) -> Result<(), String> {
        // A unit systemd never loaded (its file written, the reload not
        // reached) is stopped already: `systemctl stop` exits 5 and says
        // "not loaded". Either is taken as stopped.
        match run(&["stop", unit], self.restart_within) {
            Err(why) if not_loaded(&why) => Ok(()),
            done => done,
        }
    }

    fn last_line(&mut self, unit: &str) -> Option<String> {
        let (_, at, before) = self.restarted.iter().find(|(name, _, _)| name == unit)?;
        let since = at.duration_since(UNIX_EPOCH).ok()?.as_secs();
        let mut command = Command::new("journalctl");
        command
            .args(["-u", unit, "--since"])
            .arg(format!("@{since}"))
            .args([
                "-o",
                "json",
                "--output-fields=_SYSTEMD_UNIT,_PID,MESSAGE",
                "-n",
                "50",
                "--no-pager",
                "-q",
            ]);
        let json = output_within(&mut command, JOURNAL_WITHIN)?;
        last_own_line(&json, unit, (*before != 0).then_some(*before))
    }
}

const SELF_BACK_SERVICE: &str = "lamp-update-back.service";
const SELF_BACK_TIMER: &str = "lamp-update-back.timer";

/// Only the selected recovery properties are checked, not arbitrary unit
/// overrides or changes made after this snapshot. The oneshot service need
/// not be active: its timer is what must be armed.
fn self_recovery_show(text: &str) -> Result<(), String> {
    let mut records = Vec::new();
    let mut record = BTreeMap::new();
    for line in text.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            if !record.is_empty() {
                records.push(std::mem::take(&mut record));
            }
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or("self recovery: malformed systemctl property")?;
        if !matches!(
            key,
            "Id" | "LoadState" | "UnitFileState" | "ActiveState" | "SubState" | "Unit"
        ) {
            return Err(format!(
                "self recovery: unexpected systemctl property {}",
                clean_line(key)
            ));
        }
        if record.insert(key, value).is_some() {
            return Err(format!("self recovery: duplicate {key} property"));
        }
    }
    let mut units = BTreeMap::new();
    for record in records {
        let unit = record
            .get("Id")
            .copied()
            .ok_or("self recovery: missing Id property")?;
        if !matches!(unit, SELF_BACK_SERVICE | SELF_BACK_TIMER) {
            return Err(format!(
                "self recovery: unexpected unit {}",
                clean_line(unit)
            ));
        }
        if units.insert(unit, record).is_some() {
            return Err(format!("self recovery: duplicate {unit} record"));
        }
    }
    for unit in [SELF_BACK_SERVICE, SELF_BACK_TIMER] {
        let record = units
            .get(unit)
            .ok_or_else(|| format!("self recovery: missing {unit} record"))?;
        let field = |name| {
            record
                .get(name)
                .copied()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| format!("self recovery: {unit} is missing {name}"))
        };
        let loaded = field("LoadState")?;
        if loaded != "loaded" {
            return Err(format!(
                "self recovery: {unit} is not loaded (LoadState={})",
                clean_line(loaded)
            ));
        }
        let enabled = field("UnitFileState")?;
        if unit == SELF_BACK_SERVICE {
            // A timer-started service is normally static, not enabled.
            // Conservatively refuse runtime-only enablement/linking and
            // generated/transient definitions, as well as masked or unknown
            // states. Accepted installation states do not certify where the
            // effective FragmentPath is stored or arbitrary unit overrides.
            if !matches!(
                enabled,
                "static" | "disabled" | "enabled" | "linked" | "indirect" | "alias"
            ) {
                return Err(format!(
                    "self recovery: {unit} has a masked, runtime-only, generated, transient or unknown UnitFileState ({})",
                    clean_line(enabled)
                ));
            }
            continue;
        }
        if enabled != "enabled" {
            return Err(format!(
                "self recovery: {unit} is not persistently enabled (UnitFileState={})",
                clean_line(enabled)
            ));
        }
        let active = field("ActiveState")?;
        let sub = field("SubState")?;
        if active != "active" || !matches!(sub, "waiting" | "running") {
            return Err(format!(
                "self recovery: {unit} is not armed (ActiveState={}, SubState={})",
                clean_line(active),
                clean_line(sub)
            ));
        }
        if field("Unit")? != SELF_BACK_SERVICE {
            return Err(format!(
                "self recovery: {unit} does not activate {SELF_BACK_SERVICE}"
            ));
        }
    }
    Ok(())
}

/// The longest a `systemctl restart` may take by default (the body folds
/// down before it stops, up to 25 s); install's own `systemctl` steps and
/// Harness's service steps are given as long.
pub const RESTART_WITHIN: Duration = Duration::from_secs(120);

/// The longest `journalctl` has to say a unit's last line: a put-back waits
/// for it, and must never wait without end.
pub const JOURNAL_WITHIN: Duration = Duration::from_secs(5);

/// The complete UTF-8 response, at most 1 MiB, if `command` ends successfully
/// within `within`; otherwise none, never a truncated or partial response.
/// Reads stay bounded when descendants retain stdout. Error cleanup stops
/// the owned process group before reaping its leader.
pub fn output_within(command: &mut Command, within: Duration) -> Option<String> {
    let bytes = query::output_within(command, Instant::now() + within).ok()?;
    String::from_utf8(bytes).ok()
}

/// What `command` printed, if it ended well within `within`, both of its
/// pipes read to their end by then, its process group owned throughout
/// (`query::output_and_stderr_within`). Otherwise an error says why: it could
/// not start, it ran on or a child it left held its pipes (the group is
/// killed), its output passed 1 MiB, or it failed, in its own trimmed words
/// on stderr. A stalled query never holds its caller past `within`.
fn show_within(command: &mut Command, within: Duration) -> Result<String, String> {
    let stdout = query::output_and_stderr_within(command, Instant::now() + within).map_err(
        |error| match error.kind() {
            io::ErrorKind::TimedOut => format!(
                "systemctl did not answer within {} s: it ran on, or a child it left held its output",
                within.as_secs()
            ),
            // Its own trimmed stderr, or its exit status.
            io::ErrorKind::Other => error.to_string(),
            _ => format!("systemctl: {error}"),
        },
    )?;
    Ok(String::from_utf8_lossy(&stdout).into_owned())
}

/// The longest line a round's outcome carries of a part's journal.
pub const LINE_MOST: usize = 200;

/// The last line in `journalctl -o json` output that `unit`'s own processes
/// wrote (`_SYSTEMD_UNIT`): what systemd, systemd-coredump or journald said
/// about it is skipped, as is `skip`, the main process from before the
/// restart (its last words as it stopped are the old build's), and a
/// message that is not text.
pub fn last_own_line(json_lines: &str, unit: &str, skip: Option<u32>) -> Option<String> {
    let skip = skip.map(|pid| pid.to_string());
    json_lines
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|entry| entry["_SYSTEMD_UNIT"] == unit)
        .filter(|entry| skip.as_deref().is_none_or(|pid| entry["_PID"] != pid))
        .filter_map(|entry| entry["MESSAGE"].as_str().map(clean_line))
        .rfind(|line| !line.is_empty())
}

/// A journal line as an outcome may carry it: control characters gone,
/// trimmed, and at most [`LINE_MOST`] characters, cut with "…".
pub fn clean_line(line: &str) -> String {
    let line: String = line
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let line = line.trim();
    if line.chars().count() <= LINE_MOST {
        return line.to_owned();
    }
    let mut cut: String = line.chars().take(LINE_MOST - 1).collect();
    cut.push('…');
    cut
}

/// Whether `run`'s error is systemctl's for a unit it has not loaded: its
/// exit status 5, or its own words.
fn not_loaded(why: &str) -> bool {
    why.contains(": exit status: 5:") || why.contains("not loaded")
}

/// `systemctl` with these words, waiting at most `within` for it.
/// How long a start of the writer's one-shot is waited for: past its own
/// `TimeoutStartSec=120`.
const WRITER_WITHIN: Duration = Duration::from_secs(150);

fn run(words: &[&str], within: Duration) -> Result<(), String> {
    let mut child = Command::new("systemctl")
        .args(words)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("systemctl: {error}"))?;
    let until = Instant::now() + within;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                let mut why = String::new();
                if let Some(mut stderr) = child.stderr.take() {
                    let _ = io::Read::read_to_string(&mut stderr, &mut why);
                }
                return Err(format!(
                    "systemctl {}: {status}: {}",
                    words.join(" "),
                    why.trim()
                ));
            }
            Ok(None) if Instant::now() < until => thread::sleep(Duration::from_millis(100)),
            Ok(None) => {
                // The job goes on in systemd; only the waiting ends.
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "systemctl {} did not finish within {} s",
                    words.join(" "),
                    within.as_secs()
                ));
            }
            Err(error) => return Err(format!("systemctl: {error}")),
        }
    }
}

/// The records of `systemctl show`: `Key=value` lines, a blank line between
/// two units.
pub fn parse_show(text: &str) -> Vec<UnitState> {
    let mut units = Vec::new();
    for record in text.split("\n\n") {
        let mut unit = UnitState::default();
        for line in record.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key {
                "Id" => unit.unit = value.to_owned(),
                "ActiveState" => unit.active = value.to_owned(),
                "SubState" => unit.sub = value.to_owned(),
                "MainPID" => unit.pid = value.parse().unwrap_or(0),
                "NRestarts" => unit.restarts = value.parse().unwrap_or(0),
                _ => {}
            }
        }
        if !unit.unit.is_empty() {
            units.push(unit);
        }
    }
    units
}

/// The real bus: what the voice says, and the lamp's health.
pub struct BusEars {
    bus: Option<Bus>,
    state: Option<Subscriber>,
    mute: Option<Subscriber>,
    speech: Vec<Subscriber>,
}

/// The name this program subscribes under.
pub(crate) const NAME: &str = "update";

impl BusEars {
    /// Subscribed to `voice/state`, `voice/mute`, `voice/heard` and
    /// `voice/said`. A bus
    /// that cannot be used is said once, and then nothing is heard: the
    /// voice can then not be judged quiet.
    pub fn new(root: &Path, say: &mut dyn FnMut(&str)) -> Self {
        let bus = match Bus::new(root) {
            Ok(bus) => bus,
            Err(error) => {
                say(&format!(
                    "the bus at {} cannot be used: {error}",
                    root.display()
                ));
                return Self {
                    bus: None,
                    state: None,
                    mute: None,
                    speech: Vec::new(),
                };
            }
        };
        let mut subscribe =
            |topic: &str, type_name: &str| match bus.subscribe_typed(topic, NAME, type_name) {
                Ok(subscriber) => Some(subscriber),
                Err(error) => {
                    say(&format!("{topic} cannot be subscribed to: {error}"));
                    None
                }
            };
        let state = subscribe(VoiceState::TOPIC, VoiceState::TYPE);
        let mute = subscribe(VoiceMute::TOPIC, VoiceMute::TYPE);
        let speech = Speech::TOPICS
            .iter()
            .filter_map(|topic| subscribe(topic, Speech::TYPE))
            .collect();
        Self {
            bus: Some(bus),
            state,
            mute,
            speech,
        }
    }
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

impl Ears for BusEars {
    fn listen(&mut self, wait: Duration) -> Vec<Heard> {
        let until = Instant::now() + wait;
        let mut heard = Vec::new();
        loop {
            let left = until.saturating_duration_since(Instant::now());
            match &mut self.state {
                // The state comes once a second: waiting on it wakes often.
                Some(state) => {
                    if let Ok(Some(message)) = state.recv(left.min(Duration::from_millis(250))) {
                        // The kept value of a voice that stopped long ago
                        // says nothing of now.
                        let age = unix_ms().saturating_sub(message.header.at_unix_ms);
                        if age <= crate::quiet::STATE_LASTS.as_millis() as u64
                            && let Ok(said) = message.parse::<VoiceState>()
                        {
                            heard.push(Heard::State(said.state));
                        }
                    }
                }
                None => thread::sleep(left.min(Duration::from_millis(250))),
            }
            // A muted voice says no state, only its mute, once a second
            // (#3770): read as the state is, a kept old one or one that
            // does not parse heard as nothing.
            if let Some(mute) = &mut self.mute {
                while let Ok(Some(message)) = mute.recv(Duration::ZERO) {
                    let age = unix_ms().saturating_sub(message.header.at_unix_ms);
                    if age <= crate::quiet::STATE_LASTS.as_millis() as u64
                        && let Ok(said) = message.parse::<VoiceMute>()
                        && said.check().is_ok()
                    {
                        heard.push(Heard::Mute(said));
                    }
                }
            }
            for speech in &mut self.speech {
                while let Ok(Some(_)) = speech.recv(Duration::ZERO) {
                    heard.push(Heard::Speech);
                }
            }
            if Instant::now() >= until {
                return heard;
            }
        }
    }

    fn health(&mut self) -> Option<Health> {
        let message = self.bus.as_ref()?.latest(Health::TOPIC).ok()??;
        if message.expired {
            return None;
        }
        let health: Health = message.parse().ok()?;
        health.check().ok()?;
        Some(health)
    }
}

/// No bus at all: hears nothing, so the voice is never judged quiet, and
/// knows no health. What a look that changes nothing uses.
pub struct Deaf;

impl Ears for Deaf {
    fn listen(&mut self, wait: Duration) -> Vec<Heard> {
        thread::sleep(wait);
        Vec::new()
    }

    fn health(&mut self) -> Option<Health> {
        None
    }
}

/// The real disk.
pub struct Statvfs;

impl Disk for Statvfs {
    fn free(&self, path: &Path) -> io::Result<u64> {
        let found = rustix::fs::statvfs(path)?;
        Ok(found.f_bavail.saturating_mul(found.f_frsize))
    }
}

/// The real clock.
pub struct Monotonic(Instant);

impl Default for Monotonic {
    fn default() -> Self {
        Self(Instant::now())
    }
}

impl Clock for Monotonic {
    fn now_ms(&self) -> u64 {
        self.0.elapsed().as_millis() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stop_of_a_unit_systemd_never_loaded_is_known_by_its_status_or_its_words() {
        assert!(not_loaded(
            "systemctl stop lamp-a.service: exit status: 5: Failed to stop lamp-a.service: Unit lamp-a.service not loaded."
        ));
        assert!(not_loaded(
            "systemctl stop lamp-a.service: exit status: 5: "
        ));
        assert!(!not_loaded(
            "systemctl stop lamp-a.service: exit status: 1: Job for lamp-a.service failed."
        ));
    }

    fn sh(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        command
    }

    #[test]
    fn a_stalled_systemctl_show_is_an_error_within_its_bound() {
        let at = Instant::now();
        let why = show_within(&mut sh("sleep 30"), Duration::from_secs(1)).unwrap_err();
        assert!(why.contains("did not answer within 1 s"), "{why}");
        assert!(at.elapsed() < Duration::from_secs(3), "{:?}", at.elapsed());
    }

    #[test]
    fn a_child_holding_the_pipe_is_an_error_within_the_bound() {
        // The stand-in ends at once, well; the child it leaves keeps both
        // pipes open.
        let at = Instant::now();
        let why = show_within(
            &mut sh("sleep 30 & echo Id=lamp-voice.service"),
            Duration::from_secs(1),
        )
        .unwrap_err();
        assert!(why.contains("did not answer within 1 s"), "{why}");
        assert!(at.elapsed() < Duration::from_secs(3), "{:?}", at.elapsed());
    }

    #[test]
    fn a_child_holding_the_pipe_is_killed_with_the_query() {
        let root = tempfile::tempdir().unwrap();
        let beats = root.path().join("beats");
        // The stand-in ends at once; the child it leaves holds both pipes
        // and writes a beat every 0.1 s until it is killed.
        let why = show_within(
            sh("(while :; do date +%s >> \"$1\"; sleep 0.1; done) & echo Id=x")
                .arg("pipe-holder")
                .arg(&beats),
            Duration::from_secs(1),
        )
        .unwrap_err();
        assert!(why.contains("did not answer within 1 s"), "{why}");
        let size = || {
            std::fs::metadata(&beats)
                .map(|meta| meta.len())
                .unwrap_or(0)
        };
        let before = size();
        assert!(before > 0, "the pipe-holding child must have run");
        thread::sleep(Duration::from_millis(500));
        assert_eq!(size(), before, "the child outlived its query");
    }

    #[test]
    fn a_failed_query_says_its_trimmed_stderr_and_a_good_one_its_stdout() {
        assert_eq!(
            show_within(
                &mut sh("echo '  no such unit  ' >&2; exit 1"),
                JOURNAL_WITHIN
            ),
            Err("no such unit".to_owned())
        );
        assert_eq!(
            show_within(&mut sh("exit 3"), JOURNAL_WITHIN),
            Err("update query ended with exit status: 3".to_owned())
        );
        let text = show_within(
            &mut sh("printf 'Id=a.service\\nActiveState=active\\n'; echo noise >&2"),
            JOURNAL_WITHIN,
        )
        .unwrap();
        assert_eq!(parse_show(&text)[0].active, "active");
        // More than a pipe holds, on both pipes, is read while it runs.
        let big = show_within(
            &mut sh("head -c 200000 /dev/zero; head -c 200000 /dev/zero >&2"),
            JOURNAL_WITHIN,
        )
        .unwrap();
        assert_eq!(big.len(), 200000);
        // Past 1 MiB on stdout is refused, not cut.
        let why = show_within(&mut sh("head -c 1048577 /dev/zero"), JOURNAL_WITHIN).unwrap_err();
        assert!(
            why.starts_with("systemctl: ") && why.contains("1 MiB"),
            "{why}"
        );
    }

    const BACK_SHOW: &str = "Id=lamp-update-back.service\nLoadState=loaded\nUnitFileState=static\nActiveState=inactive\nSubState=dead\n";
    const TIMER_SHOW: &str = "Id=lamp-update-back.timer\nLoadState=loaded\nUnitFileState=enabled\nActiveState=active\nSubState=waiting\nUnit=lamp-update-back.service\n";

    fn ready_recovery_show() -> String {
        format!("{BACK_SHOW}\n{TIMER_SHOW}")
    }

    #[test]
    fn self_recovery_accepts_an_armed_timer_and_an_inactive_service() {
        assert_eq!(self_recovery_show(&ready_recovery_show()), Ok(()));
        // Record order is not significant, and a timer can be running its
        // service at the instant it is asked.
        assert_eq!(
            self_recovery_show(&format!(
                "{}\n{BACK_SHOW}",
                TIMER_SHOW.replace("SubState=waiting", "SubState=running")
            )),
            Ok(())
        );
    }

    #[test]
    fn self_recovery_refuses_a_timer_not_armed_persistently() {
        for (before, after) in [
            ("LoadState=loaded", "LoadState=not-found"),
            ("LoadState=loaded", "LoadState=masked"),
            ("UnitFileState=enabled", "UnitFileState=enabled-runtime"),
            ("UnitFileState=enabled", "UnitFileState=disabled"),
            ("UnitFileState=enabled", "UnitFileState=masked-runtime"),
            ("UnitFileState=enabled", "UnitFileState=future-state"),
            ("ActiveState=active", "ActiveState=inactive"),
            ("ActiveState=active", "ActiveState=failed"),
            ("ActiveState=active", "ActiveState=future-state"),
            ("SubState=waiting", "SubState=elapsed"),
            ("SubState=waiting", "SubState=future-state"),
            ("Unit=lamp-update-back.service", "Unit=another-back.service"),
        ] {
            let text = format!("{BACK_SHOW}\n{}", TIMER_SHOW.replace(before, after));
            let why = self_recovery_show(&text).unwrap_err();
            assert!(why.contains(SELF_BACK_TIMER), "{after}: {why}");
        }
    }

    #[test]
    fn self_recovery_refuses_an_unloaded_masked_or_ephemeral_service() {
        for (before, after) in [
            ("LoadState=loaded", "LoadState=not-found"),
            ("LoadState=loaded", "LoadState=masked"),
            ("LoadState=loaded", "LoadState=future-state"),
            ("UnitFileState=static", "UnitFileState=masked"),
            ("UnitFileState=static", "UnitFileState=masked-runtime"),
            ("UnitFileState=static", "UnitFileState=enabled-runtime"),
            ("UnitFileState=static", "UnitFileState=linked-runtime"),
            ("UnitFileState=static", "UnitFileState=generated"),
            ("UnitFileState=static", "UnitFileState=transient"),
            ("UnitFileState=static", "UnitFileState=bad"),
            ("UnitFileState=static", "UnitFileState=future-state"),
        ] {
            let text = format!("{}\n{TIMER_SHOW}", BACK_SHOW.replace(before, after));
            let why = self_recovery_show(&text).unwrap_err();
            assert!(why.contains(SELF_BACK_SERVICE), "{after}: {why}");
        }
    }

    #[test]
    fn self_recovery_requires_one_of_each_record_and_required_property() {
        for (record, other, fields) in [
            (
                BACK_SHOW,
                TIMER_SHOW,
                &["Id", "LoadState", "UnitFileState"][..],
            ),
            (
                TIMER_SHOW,
                BACK_SHOW,
                &[
                    "Id",
                    "LoadState",
                    "UnitFileState",
                    "ActiveState",
                    "SubState",
                    "Unit",
                ][..],
            ),
        ] {
            assert!(self_recovery_show(other).is_err(), "missing record");
            let duplicate = format!("{record}\n{other}\n{record}");
            assert!(
                self_recovery_show(&duplicate)
                    .unwrap_err()
                    .contains("duplicate"),
                "duplicate record"
            );
            for field in fields {
                let prefix = format!("{field}=");
                let line = record
                    .lines()
                    .find(|line| line.starts_with(&prefix))
                    .unwrap();
                let missing = record.replace(&format!("{line}\n"), "");
                assert!(
                    self_recovery_show(&format!("{missing}\n{other}")).is_err(),
                    "missing {field}"
                );
                let empty = record.replace(line, &prefix);
                assert!(
                    self_recovery_show(&format!("{empty}\n{other}")).is_err(),
                    "empty {field}"
                );
                assert!(
                    self_recovery_show(&format!("{record}{line}\n\n{other}"))
                        .unwrap_err()
                        .contains("duplicate"),
                    "duplicate {field}"
                );
            }
        }
    }

    #[test]
    fn self_recovery_refuses_malformed_or_unexpected_query_records() {
        for text in [
            String::new(),
            format!("{BACK_SHOW}\n{TIMER_SHOW}not a property\n"),
            format!("{BACK_SHOW}\n{TIMER_SHOW}Unexpected=loaded\n"),
            ready_recovery_show().replace("Id=lamp-update-back.timer", "Id=unrelated.timer"),
        ] {
            assert!(self_recovery_show(&text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn systemctl_show_is_read_unit_by_unit() {
        let text = "Id=lamp-voice.service\nActiveState=active\nSubState=running\nMainPID=812\nNRestarts=2\n\nId=lamp-body.service\nActiveState=activating\nSubState=auto-restart\nMainPID=0\nNRestarts=1\n";
        let units = parse_show(text);
        assert_eq!(units.len(), 2);
        assert_eq!(units[0].unit, "lamp-voice.service");
        assert_eq!((units[0].pid, units[0].restarts), (812, 2));
        assert!(units[0].running());
        assert_eq!(units[1].sub, "auto-restart");
        assert!(!units[1].running());
    }

    #[test]
    fn the_real_disk_has_room() {
        assert!(Statvfs.free(Path::new("/")).unwrap() > 0);
    }

    #[test]
    fn the_bus_hears_the_voice_and_not_an_old_state() {
        // Short: a socket's path holds 103 bytes, and the Mac's own temporary
        // folder is long.
        let root = tempfile::Builder::new()
            .prefix("lu-")
            .tempdir_in("/tmp")
            .unwrap();
        let bus = Bus::new(root.path()).unwrap();
        // A state kept long ago: a voice that has stopped.
        let mut state = bus
            .publisher_declared(VoiceState::TOPIC, &VoiceState::declaration(), "voice")
            .unwrap();
        let mut said = |_: &str| {};
        let mut ears = BusEars::new(root.path(), &mut said);
        let mut heard_pub = bus
            .publisher_declared("voice/heard", &Speech::declaration(), "voice")
            .unwrap();
        // Subscribed now; a publisher finds a new subscriber within 200 ms.
        thread::sleep(Duration::from_millis(300));
        heard_pub
            .publish_json(
                2,
                &Speech {
                    text: "hello".into(),
                    finished: true,
                    locale: None,
                    request: None,
                },
            )
            .unwrap();
        // The state last, just before listening: one older than
        // `quiet::STATE_LASTS` (3 s) is not heard, so a stalled test thread
        // between its publish and the listening must not age it.
        state
            .publish_json(
                1,
                &VoiceState {
                    state: lamp_msgs::Voice::Speaking,
                },
            )
            .unwrap();
        let heard = ears.listen(Duration::from_millis(500));
        assert!(
            heard.contains(&Heard::State(lamp_msgs::Voice::Speaking)),
            "{heard:?}"
        );
        assert!(heard.contains(&Heard::Speech), "{heard:?}");
        assert_eq!(ears.health(), None);
    }

    #[test]
    fn the_bus_hears_a_muted_voice_and_not_a_mute_it_cannot_read() {
        let root = tempfile::Builder::new()
            .prefix("lu-")
            .tempdir_in("/tmp")
            .unwrap();
        let bus = Bus::new(root.path()).unwrap();
        let mut mute = bus
            .publisher_declared(VoiceMute::TOPIC, &VoiceMute::declaration(), "voice")
            .unwrap();
        let mut said = |_: &str| {};
        let mut ears = BusEars::new(root.path(), &mut said);
        thread::sleep(Duration::from_millis(300));
        let mutes = |heard: &[Heard]| {
            heard
                .iter()
                .filter(|heard| matches!(heard, Heard::Mute(_)))
                .count()
        };
        // Neither a mute that does not parse nor one that is not a mute's.
        for wrong in [
            serde_json::json!({"muted": "yes"}),
            serde_json::json!({"muted": false, "reason": "sleep"}),
        ] {
            mute.publish_json(1, &wrong).unwrap();
            let heard = ears.listen(Duration::from_millis(500));
            assert_eq!(mutes(&heard), 0, "{wrong}: {heard:?}");
        }
        let asleep = VoiceMute {
            muted: true,
            reason: Some(lamp_msgs::MuteReason::Sleep),
        };
        mute.publish_json(2, &asleep).unwrap();
        let heard = ears.listen(Duration::from_millis(500));
        assert!(heard.contains(&Heard::Mute(asleep)), "{heard:?}");
    }
}
