//! The only things the helper asks of the system besides files: systemctl,
//! and whether a unit's cgroup has a process left. Behind a trait so the
//! order of a remove is tested with a fake. Nothing here runs in a test.
use std::process::Command;

pub const SYSTEMCTL: &str = "/usr/bin/systemctl";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verb {
    Start,
    /// Starts it, or restarts it if it runs: an update's way to load new code.
    Restart,
    Stop,
    ResetFailed,
}

impl Verb {
    pub fn word(self) -> &'static str {
        match self {
            Verb::Start => "start",
            Verb::Restart => "restart",
            Verb::Stop => "stop",
            Verb::ResetFailed => "reset-failed",
        }
    }
}

/// The command line, whole: the program by its absolute path, and `--`
/// before the unit so a name can never be read as an option.
pub fn argv(verb: Verb, unit: &str) -> Vec<String> {
    vec![
        SYSTEMCTL.into(),
        verb.word().into(),
        "--".into(),
        unit.into(),
    ]
}

pub trait Effects {
    fn systemctl(&mut self, verb: Verb, unit: &str) -> Result<(), String>;
    /// Whether the unit's cgroup has no process left (or has none at all).
    fn unit_empty(&mut self, unit: &str) -> Result<bool, String>;
    /// A short wait between two `unit_empty` asks.
    fn pause(&mut self) {}
}

/// Whether a unit has no process left, from what `systemctl show
/// --property=ControlGroup --value` said and the unit's `cgroup.events`
/// (`populated 0` covers its children too). A failed `show`, a path that is
/// not a cgroup's, or an events file that cannot be read or understood is an
/// error and never "empty": a failed read must not let a remove delete the
/// data under a running app. A unit with no cgroup (never started, or gone)
/// and an events file that is gone are empty.
pub fn judge_unit_empty(
    show_succeeded: bool,
    show_stdout: &str,
    events: &dyn Fn(&str) -> std::io::Result<String>,
) -> Result<bool, String> {
    if !show_succeeded {
        return Err("systemctl show failed".into());
    }
    let group = show_stdout.trim();
    if group.is_empty() {
        return Ok(true);
    }
    if !group.starts_with('/') || group.split('/').any(|part| part == "..") || group.contains('\n')
    {
        return Err(format!("not a cgroup path: {group:?}"));
    }
    match events(group) {
        Ok(text) => match text
            .lines()
            .find_map(|line| line.strip_prefix("populated "))
        {
            Some("0") => Ok(true),
            Some("1") => Ok(false),
            other => Err(format!("cgroup.events says {other:?}")),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(format!("{group}: {error}")),
    }
}

/// The real thing, for the helper itself (piece B, not wired in A).
pub struct RealEffects;

impl Effects for RealEffects {
    fn systemctl(&mut self, verb: Verb, unit: &str) -> Result<(), String> {
        let line = argv(verb, unit);
        let status = Command::new(&line[0])
            .args(&line[1..])
            .env_clear()
            .status()
            .map_err(|error| format!("{}: {error}", line[0]))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("{} {} {unit}: {status}", line[0], verb.word()))
        }
    }

    fn unit_empty(&mut self, unit: &str) -> Result<bool, String> {
        let output = Command::new(SYSTEMCTL)
            .args(["show", "--property=ControlGroup", "--value", "--", unit])
            .env_clear()
            .output()
            .map_err(|error| format!("{SYSTEMCTL}: {error}"))?;
        judge_unit_empty(
            output.status.success(),
            &String::from_utf8_lossy(&output.stdout),
            &|group| std::fs::read_to_string(format!("/sys/fs/cgroup{group}/cgroup.events")),
        )
        .map_err(|why| format!("{unit}: {why}"))
    }

    fn pause(&mut self) {
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}
