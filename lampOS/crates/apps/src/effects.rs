//! The only things the helper asks of the system besides files: systemctl,
//! and whether a unit's cgroup has a process left. Behind a trait so the
//! order of a remove is tested with a fake. Nothing here runs in a test.
use std::process::Command;

pub const SYSTEMCTL: &str = "/usr/bin/systemctl";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verb {
    Start,
    Stop,
    ResetFailed,
}

impl Verb {
    pub fn word(self) -> &'static str {
        match self {
            Verb::Start => "start",
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
        let group = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if group.is_empty() || group.contains("..") || !group.starts_with('/') {
            return Ok(true);
        }
        match std::fs::read_to_string(format!("/sys/fs/cgroup{group}/cgroup.procs")) {
            Ok(procs) => Ok(procs.trim().is_empty()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
            Err(error) => Err(format!("{group}: {error}")),
        }
    }

    fn pause(&mut self) {
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}
