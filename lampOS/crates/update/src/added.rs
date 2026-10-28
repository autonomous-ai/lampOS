//! A product unit a round adds to a lamp that lacks it (#3689, item 2b;
//! docs/updates.md, "A part added over the air"): how it is enabled, and
//! whether the lamp has the users it runs as.
//!
//! A round enables an added unit as `systemctl enable` would, by writing
//! the links its `[Install]` section asks for, and nothing else. Those links
//! are files under `/etc/systemd/system`, which a round may write, and they
//! are removed again on the way back. A unit with no `[Install]` is never
//! enabled: a template is started by its socket, a timed service by its
//! timer.
//!
//! A round cannot make a user or a group: `/etc/passwd` is not its to
//! write. A unit that runs as one the lamp lacks is not added, and is left
//! to `lamp-update install`.
use std::{fs, path::Path};

/// Where the units are, as the lamp names it.
pub const UNITS: &str = "/etc/systemd/system";

/// The links that enable `name` as its `[Install]` asks: each
/// `WantedBy=X` is `/etc/systemd/system/X.wants/<name>`, each `RequiredBy=X`
/// `X.requires/<name>`. None for a unit with no `[Install]`. An `[Install]`
/// this program does not write (`Alias=`, `Also=`, `DefaultInstance=`, a
/// template's, an `UpheldBy=`) or a target that is not a plain name is
/// refused, rather than half enabled.
pub fn wants(name: &str, text: &str) -> Result<Vec<String>, String> {
    let mut within = false;
    let mut links = Vec::new();
    for line in text.lines().map(str::trim) {
        if let Some(section) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            within = section == "Install";
            continue;
        }
        if !within || line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let folder = match key.trim() {
            "WantedBy" => "wants",
            "RequiredBy" => "requires",
            other => {
                return Err(format!(
                    "{name}: [Install] {other}= is not written by a round"
                ));
            }
        };
        if name.contains('@') {
            return Err(format!("{name}: a template is never enabled"));
        }
        for target in value.split_whitespace() {
            if !is_target(target) {
                return Err(format!(
                    "{name}: {target:?} is not a unit a round enables it by"
                ));
            }
            let link = format!("{UNITS}/{target}.{folder}/{name}");
            if !links.contains(&link) {
                links.push(link);
            }
        }
    }
    Ok(links)
}

/// The unit a link of [`wants`] enables `name` by: `lampos.target`.
pub fn target(link: &str) -> Option<&str> {
    let folder = Path::new(link).parent()?.file_name()?.to_str()?;
    folder
        .strip_suffix(".wants")
        .or_else(|| folder.strip_suffix(".requires"))
}

/// The users and groups a unit runs as (`User=`, `Group=`,
/// `SupplementaryGroups=`) that the lamp under `root` lacks, each as
/// `user x` or `group x`. Root, and a unit that names none, need nothing.
pub fn missing(root: &Path, text: &str) -> Vec<String> {
    let names = |file: &str| -> Vec<String> {
        fs::read_to_string(root.join(file))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.split(':').next())
            .map(str::to_owned)
            .collect()
    };
    let (users, groups) = (names("etc/passwd"), names("etc/group"));
    let mut missing = Vec::new();
    for line in text.lines().map(str::trim) {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (kind, known) = match key.trim() {
            "User" => ("user", &users),
            "Group" | "SupplementaryGroups" => ("group", &groups),
            _ => continue,
        };
        for name in value.split_whitespace().filter(|name| *name != "root") {
            let line = format!("{kind} {name}");
            if !known.iter().any(|known| known == name) && !missing.contains(&line) {
                missing.push(line);
            }
        }
    }
    missing
}

/// A unit's plain name: letters, digits and `-_.`, then a known suffix.
fn is_target(name: &str) -> bool {
    name.len() <= 64
        && [".target", ".service", ".socket", ".timer", ".path"]
            .iter()
            .any(|suffix| {
                name.strip_suffix(suffix)
                    .is_some_and(|stem| !stem.is_empty())
            })
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}

#[cfg(test)]
mod tests;
