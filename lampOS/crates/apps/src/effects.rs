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
    /// `systemctl clean --what=state`: systemd removes the unit's own state
    /// (a dynamic user's private folder and its link); the helper writes
    /// nothing there. First real on a lamp.
    Clean,
}

impl Verb {
    pub fn word(self) -> &'static str {
        match self {
            Verb::Start => "start",
            Verb::Restart => "restart",
            Verb::Stop => "stop",
            Verb::ResetFailed => "reset-failed",
            Verb::Clean => "clean",
        }
    }
}

/// The command line, whole: the program by its absolute path, and `--`
/// before the unit so a name can never be read as an option.
pub fn argv(verb: Verb, unit: &str) -> Vec<String> {
    let mut line = vec![SYSTEMCTL.into(), verb.word().into()];
    if verb == Verb::Clean {
        line.push("--what=state".into());
    }
    line.push("--".into());
    line.push(unit.into());
    line
}

/// What `systemctl`'s exit status means for a verb. `systemctl` says 5 when
/// the unit is not loaded: for a stop, a reset of the failed state or a clean of
/// its state, a unit that is already gone IS the wanted end, not an
/// error (a remove cut at `reset-failed` because the unit had unloaded left
/// the app half removed, 18 Nov, on 0c89). A start or a restart of a unit that
/// is not loaded is an error, as is every other exit.
pub fn judge_status(verb: Verb, unit: &str, code: Option<i32>) -> Result<(), String> {
    let gone_is_done = matches!(verb, Verb::Stop | Verb::ResetFailed | Verb::Clean);
    match code {
        Some(0) => Ok(()),
        Some(5) if gone_is_done => Ok(()),
        other => Err(format!(
            "{SYSTEMCTL} {} {unit}: {}",
            verb.word(),
            other.map_or_else(|| "killed by a signal".to_owned(), |c| format!("exit {c}"))
        )),
    }
}

pub trait Effects {
    fn systemctl(&mut self, verb: Verb, unit: &str) -> Result<(), String>;
    /// Whether the unit's cgroup has no process left (or has none at all).
    fn unit_empty(&mut self, unit: &str) -> Result<bool, String>;
    /// Tells lamp-control the app is gone (`{"app_forget": NAME}`), so what it
    /// kept of the app goes too. Called only after both unit variants have
    /// stopped; an error leaves the remove cut, to be resumed.
    fn forget(&mut self, name: &str) -> Result<(), String>;
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
    fn forget(&mut self, name: &str) -> Result<(), String> {
        crate::client::AdminYes::product().forget(name)
    }

    fn systemctl(&mut self, verb: Verb, unit: &str) -> Result<(), String> {
        let line = argv(verb, unit);
        let status = Command::new(&line[0])
            .args(&line[1..])
            .env_clear()
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .status()
            .map_err(|error| format!("{}: {error}", line[0]))?;
        judge_status(verb, unit, status.code())
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
