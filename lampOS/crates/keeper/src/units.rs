//! The lamp's units as systemd has them, and how the keeper asks and
//! commands systemd: through `systemctl`, behind a trait so the tests use
//! a stand-in.
use lamp_msgs::Health;
use std::process::Command;

/// The unit that is the whole lamp.
pub const LAMP: &str = "lampos.target";
/// What `status` asks systemd about: every lampOS part, and the lamp.
pub const PATTERNS: [&str; 2] = ["lamp-*", LAMP];
/// The properties `status` reads.
const PROPERTIES: &str = "--property=Id,LoadState,ActiveState,SubState,ExecStart";

/// The unit of a part, or of the lamp: `body` is `lamp-body.service`.
pub fn unit_of(part: &str) -> String {
    if part == "lamp" {
        LAMP.to_owned()
    } else {
        format!("lamp-{part}.service")
    }
}

/// The part a unit is, if it is one: `lamp-body.service` is `body`.
pub fn part_of(unit: &str) -> Option<&str> {
    unit.strip_prefix("lamp-")?.strip_suffix(".service")
}

/// A part's name as a person may give it on the command line: plain, so it
/// can name nothing but a lampOS unit.
pub fn is_part_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

pub trait Systemd {
    /// Whether the unit is running now.
    fn active(&mut self, unit: &str) -> bool;
    /// `systemctl <verb> <unit>`: start, stop or restart.
    fn command(&mut self, verb: &str, unit: &str) -> Result<(), String>;
    /// `systemctl show` of the units the patterns match, as text.
    fn show(&mut self, patterns: &[&str]) -> Result<String, String>;
    /// `systemctl link --force <file>`: a unit file a release carries.
    fn link(&mut self, file: &std::path::Path) -> Result<(), String> {
        Err(format!("cannot link {}", file.display()))
    }
    /// `systemctl daemon-reload`.
    fn reload(&mut self) -> Result<(), String> {
        Err("cannot reload".into())
    }
    /// `systemctl enable <unit>`: a part is wanted by the lamp through its
    /// own `WantedBy=lampos.target`, which only enabling makes so.
    fn enable(&mut self, unit: &str) -> Result<(), String> {
        Err(format!("cannot enable {unit}"))
    }
    /// `systemctl mask <unit>`: the unit cannot be started until unmasked.
    fn mask(&mut self, unit: &str) -> Result<(), String> {
        Err(format!("cannot mask {unit}"))
    }
}

/// The real one.
pub struct Systemctl;

/// Run `systemctl` with these words and say why it failed, if it did.
fn systemctl(words: &[&std::ffi::OsStr]) -> Result<(), String> {
    let output = Command::new("systemctl")
        .args(words)
        .output()
        .map_err(|error| format!("systemctl: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

impl Systemd for Systemctl {
    fn link(&mut self, file: &std::path::Path) -> Result<(), String> {
        systemctl(&["link".as_ref(), "--force".as_ref(), file.as_os_str()])
    }

    fn reload(&mut self) -> Result<(), String> {
        systemctl(&["daemon-reload".as_ref()])
    }

    fn mask(&mut self, unit: &str) -> Result<(), String> {
        systemctl(&["mask".as_ref(), unit.as_ref()])
    }

    fn enable(&mut self, unit: &str) -> Result<(), String> {
        systemctl(&["enable".as_ref(), unit.as_ref()])
    }

    fn active(&mut self, unit: &str) -> bool {
        Command::new("systemctl")
            .args(["is-active", "--quiet", unit])
            .status()
            .is_ok_and(|status| status.success())
    }

    fn command(&mut self, verb: &str, unit: &str) -> Result<(), String> {
        let output = Command::new("systemctl")
            .args([verb, unit])
            .output()
            .map_err(|error| format!("systemctl: {error}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
        }
    }

    fn show(&mut self, patterns: &[&str]) -> Result<String, String> {
        let output = Command::new("systemctl")
            .arg("show")
            .arg(PROPERTIES)
            .args(patterns)
            .output()
            .map_err(|error| format!("systemctl: {error}"))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
        }
    }
}

/// One unit as `systemctl show` describes it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Unit {
    pub id: String,
    pub load: String,
    pub active: String,
    pub sub: String,
    /// The build: the folder its program is in, when that is a hash, as the
    /// lamp installs them (`/opt/lampOS/body/<sha256>/lamp-body`).
    pub build: Option<String>,
}

/// The records of `systemctl show`: `Key=value` lines, a blank line between
/// two units.
pub fn parse_show(text: &str) -> Vec<Unit> {
    let mut units = Vec::new();
    for record in text.split("\n\n") {
        let mut unit = Unit::default();
        for line in record.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key {
                "Id" => unit.id = value.to_owned(),
                "LoadState" => unit.load = value.to_owned(),
                "ActiveState" => unit.active = value.to_owned(),
                "SubState" => unit.sub = value.to_owned(),
                "ExecStart" => unit.build = build_of(value),
                _ => {}
            }
        }
        if !unit.id.is_empty() {
            units.push(unit);
        }
    }
    units
}

/// `{ path=/opt/lampOS/body/<hash>/lamp-body ; argv[]=... }` to `<hash>`.
fn build_of(exec_start: &str) -> Option<String> {
    let path = exec_start.split("path=").nth(1)?.split([' ', ';']).next()?;
    let folder = std::path::Path::new(path).parent()?.file_name()?.to_str()?;
    (folder.len() >= 12 && folder.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| folder.to_owned())
}

/// What `lamp-keeper status` prints: a line for each unit, with its state,
/// its build and its part's health.
pub fn status_lines(units: &[Unit], health: Option<&Health>) -> String {
    let mut lines = String::new();
    for unit in units {
        let state = if unit.load == "loaded" {
            format!("{}/{}", unit.active, unit.sub)
        } else {
            unit.load.clone()
        };
        let build = unit.build.as_deref().map_or("-", |build| &build[..12]);
        let well = match (part_of(&unit.id), health) {
            (Some(part), Some(health)) => health
                .items
                .iter()
                .find(|item| item.name == part)
                .map(|item| {
                    if item.message.is_empty() {
                        item.level.as_str().to_owned()
                    } else {
                        format!("{}: {}", item.level.as_str(), item.message)
                    }
                })
                .unwrap_or_else(|| {
                    if health.missing.iter().any(|missing| missing == part) {
                        "missing".to_owned()
                    } else {
                        "-".to_owned()
                    }
                }),
            (Some(_), None) => "unknown".to_owned(),
            (None, _) => "-".to_owned(),
        };
        lines.push_str(&format!(
            "{:<24} {:<20} {:<12} {well}\n",
            unit.id, state, build
        ));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::{HealthItem, Level};

    /// As systemd 252 on a lamp prints it.
    const SHOW: &str = "\
Id=lamp-body.service
LoadState=loaded
ActiveState=active
SubState=running
ExecStart={ path=/opt/lampOS/body/d22c7fe890694663292020beb2639b09edfaa0141272188ba6ac58e6c24912a7/lamp-body ; argv[]=/opt/lampOS/body/d22c7fe890694663292020beb2639b09edfaa0141272188ba6ac58e6c24912a7/lamp-body --device /dev/ttyACM0 ; ignore_errors=no ; start_time=[Tue 2026-10-14 09:43:01 +07] ; stop_time=[n/a] ; pid=1234 ; code=(null) ; status=0/0 }

Id=lamp-voice.service
LoadState=loaded
ActiveState=failed
SubState=failed
ExecStart={ path=/usr/local/bin/lamp-voice ; argv[]=/usr/local/bin/lamp-voice ; ignore_errors=no ; start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }

Id=lampos.target
LoadState=not-found
ActiveState=inactive
SubState=dead
";

    #[test]
    fn the_show_text_is_read_unit_by_unit() {
        let units = parse_show(SHOW);
        assert_eq!(units.len(), 3);
        assert_eq!(
            units[0],
            Unit {
                id: "lamp-body.service".into(),
                load: "loaded".into(),
                active: "active".into(),
                sub: "running".into(),
                build: Some(
                    "d22c7fe890694663292020beb2639b09edfaa0141272188ba6ac58e6c24912a7".into()
                ),
            }
        );
        assert_eq!(units[1].active, "failed");
        assert_eq!(units[1].build, None);
        assert_eq!(units[2].load, "not-found");
        assert!(parse_show("").is_empty());
    }

    #[test]
    fn status_says_each_unit_its_build_and_its_health() {
        let health = Health {
            level: Level::Warn,
            message: "body: head_pitch is warm".into(),
            items: vec![HealthItem {
                name: "body".into(),
                level: Level::Warn,
                message: "head_pitch is warm".into(),
                age_ms: 120,
            }],
            missing: vec!["voice".into()],
            quiet_ms: 0,
        };
        let lines = status_lines(&parse_show(SHOW), Some(&health));
        let lines: Vec<_> = lines
            .lines()
            .map(|line| line.split_whitespace().collect::<Vec<_>>())
            .collect();
        assert_eq!(
            lines[0],
            [
                "lamp-body.service",
                "active/running",
                "d22c7fe89069",
                "warn:",
                "head_pitch",
                "is",
                "warm"
            ]
        );
        assert_eq!(
            lines[1],
            ["lamp-voice.service", "failed/failed", "-", "missing"]
        );
        assert_eq!(lines[2], ["lampos.target", "not-found", "-", "-"]);
        let unknown = status_lines(&parse_show(SHOW), None);
        assert!(unknown.lines().next().unwrap().ends_with("unknown"));
    }

    #[test]
    fn parts_and_units_name_each_other() {
        assert_eq!(unit_of("body"), "lamp-body.service");
        assert_eq!(unit_of("lamp"), "lampos.target");
        assert_eq!(part_of("lamp-body.service"), Some("body"));
        assert_eq!(part_of("lampos.target"), None);
        assert!(is_part_name("turn-end"));
        for refused in ["", "Body", "../x", "a b", "body.service", "x;y"] {
            assert!(!is_part_name(refused), "{refused}");
        }
    }
}
