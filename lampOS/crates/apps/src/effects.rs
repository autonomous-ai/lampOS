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

/// What `systemctl` said for a verb: its exit status and what it wrote to stderr. A unit that is not loaded
/// IS the wanted end of a stop, a reset of the failed state and a clean of its state (a remove cut at
/// `reset-failed` because the unit had unloaded left the app half removed, 18 Nov and 20 Nov, on 0c89),
/// however `systemctl` words it: exit 5 (a stop), or exit 1 with "not loaded" or "not found" (a reset, a
/// clean, and a stop on some versions, seen on systemd 255). A start or a restart of a unit that is not
/// loaded is an error, as is every other exit.
pub fn judge_status(verb: Verb, unit: &str, code: Option<i32>, said: &str) -> Result<(), String> {
    let gone_is_done = matches!(verb, Verb::Stop | Verb::ResetFailed | Verb::Clean);
    let said_gone = {
        let said = said.to_ascii_lowercase();
        said.contains("not loaded") || said.contains("not found")
    };
    match code {
        Some(0) => Ok(()),
        Some(5) if gone_is_done => Ok(()),
        Some(1) if gone_is_done && said_gone => Ok(()),
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
    /// The one cap the apps share (`lampos-apps.slice`), set at runtime before the first app
    /// starts: the slice has no unit file (no round could carry one), and `--runtime` keeps it in
    /// `/run`, set again at each boot. An error means no app is started.
    fn limit_apps(&mut self) -> Result<(), String> {
        Ok(())
    }
}

/// What the shared cap is: the slice, its ceiling (`MemoryMax`, `TasksMax`) and the shares the old slice file
/// gave (`CPUWeight`, `IOWeight`, `MemoryHigh`), so the apps never get the voice's CPU or disk share.
pub const APPS_SLICE_LIMIT: [&str; 8] = [
    "set-property",
    "--runtime",
    "lampos-apps.slice",
    "CPUWeight=50",
    "IOWeight=50",
    "MemoryHigh=192M",
    "MemoryMax=256M",
    "TasksMax=128",
];

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
        let output = Command::new(&line[0])
            .args(&line[1..])
            .env_clear()
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|error| format!("{}: {error}", line[0]))?;
        // Only the start of what it said is read: a unit name and a reason.
        let said =
            String::from_utf8_lossy(&output.stderr[..output.stderr.len().min(512)]).into_owned();
        judge_status(verb, unit, output.status.code(), &said)
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

    fn limit_apps(&mut self) -> Result<(), String> {
        let status = Command::new(SYSTEMCTL)
            .args(APPS_SLICE_LIMIT)
            .env_clear()
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .status()
            .map_err(|error| format!("{SYSTEMCTL}: {error}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!(
                "{SYSTEMCTL} {}: {status}",
                APPS_SLICE_LIMIT.join(" ")
            ))
        }
    }
}
