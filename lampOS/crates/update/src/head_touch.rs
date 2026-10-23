//! Keep the pins' head delivery paired with the units installed at the switch.
//! Settings remain signed inputs; this local projection changes only the
//! compatibility bit, and the ordinary apply journal records the actual bytes.
use crate::{
    apply::{Act, Step},
    manifest::{self, Setting},
    unit,
};
use std::{collections::BTreeMap, fs, io, path::Path};

pub(crate) const PINS: &str = "/etc/lampOS/controls/controls-pins.json";
const CONTROLS: &str = "lamp-controls.service";
const VOICE: &str = "lamp-voice.service";
const EXPRESSION: &str = "lamp-expression.service";

fn read(path: &Path) -> Result<Option<String>, String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

fn service(text: &str) -> String {
    let mut inside = false;
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.starts_with('[') {
                inside = line == "[Service]";
                return None;
            }
            if !inside || line.starts_with(['#', ';']) {
                return None;
            }
            Some(match line.split_once('=') {
                Some((key, value)) => format!("{}={}", key.trim(), value.trim()),
                None => line.to_owned(),
            })
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// The product uses literal Environment and ExecStart settings. Do not guess
// when a local override can change those: leave the installed lamp untouched.
fn no_mode_overrides(root: &Path, name: &str) -> Result<(), String> {
    for base in [
        "etc/systemd/system",
        "run/systemd/system",
        "usr/lib/systemd/system",
        "lib/systemd/system",
    ] {
        for folder in [
            "service.d".to_owned(),
            "lamp-.service.d".to_owned(),
            format!("{name}.d"),
        ] {
            let entries = match fs::read_dir(root.join(base).join(folder)) {
                Ok(entries) => entries,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(format!("head touch overrides cannot be read: {error}")),
            };
            for entry in entries {
                let path = entry.map_err(|e| e.to_string())?.path();
                if path.extension().is_none_or(|ext| ext != "conf") {
                    continue;
                }
                let text = read(&path)?.unwrap_or_default();
                if service(&text).lines().any(|line| {
                    line.ends_with('\\')
                        || line.starts_with("ExecStart=")
                        || line == "Environment="
                        || line.starts_with("EnvironmentFile=")
                        || line.starts_with("PassEnvironment=")
                        || line.starts_with("UnsetEnvironment=")
                        || (line.starts_with("Environment=")
                            && (line.contains("LAMP_HEAD_TOUCH")
                                || line.contains("LAMP_BUS_ROOT")
                                || line.contains("LAMP_PRIVACY_CONTROLS")
                                || line.contains('\\')))
                }) {
                    return Err(format!(
                        "head touch mode has a local override in {}; pair its units and pins before updating",
                        path.display()
                    ));
                }
            }
        }
    }
    Ok(())
}

fn flag(text: &str, flag: &str) -> bool {
    text.lines()
        .filter_map(|line| line.strip_prefix("ExecStart="))
        .next_back()
        .is_some_and(|line| {
            line.split_whitespace()
                .any(|word| word.trim_matches(['\'', '"']) == flag)
        })
}

fn reader(
    root: &Path,
    units: &Path,
    carried: &BTreeMap<String, String>,
    name: &str,
) -> Result<Option<String>, String> {
    no_mode_overrides(root, name)?;
    let text = match carried.get(name) {
        Some(text) => text.clone(),
        None => match read(&units.join(name))? {
            Some(text) => text,
            None => return Ok(None),
        },
    };
    let program = unit::program(&text)
        .map(|program| program.binary)
        .or_else(|| unit::linked(&text).map(|(_, binary)| binary));
    if program.as_deref() != name.strip_suffix(".service") {
        return Err(format!(
            "head touch reader {name} is not a recognized lamp unit"
        ));
    }
    let text = service(&text);
    if text.lines().any(|line| {
        line.ends_with('\\')
            || (line.starts_with("ExecStart=")
                && (line.contains(['\\', '\'', '"', '%'])
                    || line.replace("${INVOCATION_ID}", "").contains('$')))
            || (line.starts_with("Environment=")
                && (line.contains('\\') || !line.matches('"').count().is_multiple_of(2)))
            || (line.starts_with("Environment=")
                && ["LAMP_HEAD_TOUCH", "LAMP_BUS_ROOT", "LAMP_PRIVACY_CONTROLS"]
                    .iter()
                    .any(|key| line.contains(key))
                && line.contains(['\'', '\t', '%']))
            || line == "Environment="
            || line.starts_with("EnvironmentFile=")
            || line.starts_with("PassEnvironment=")
            || line.starts_with("UnsetEnvironment=")
    }) {
        return Err(format!(
            "head touch mode in {name} is not a literal unit setting"
        ));
    }
    Ok(Some(text))
}

fn argument<'a>(text: &'a str, flag: &str) -> Result<Option<&'a str>, String> {
    let mut values = text
        .lines()
        .filter_map(|line| line.strip_prefix("ExecStart="));
    let exec = values.next().unwrap_or_default();
    if values.next().is_some() {
        return Err("head touch cannot infer multiple ExecStart directives".into());
    }
    let mut words = exec.split_whitespace();
    let mut value = None;
    while let Some(word) = words.next() {
        if word == flag {
            let next = words
                .next()
                .filter(|next| !next.starts_with('-'))
                .ok_or_else(|| format!("head touch needs a value for {flag}"))?;
            if next.contains(['$', '%']) {
                return Err(format!("head touch needs a literal value for {flag}"));
            }
            if value.replace(next).is_some() {
                return Err(format!("head touch cannot infer repeated {flag}"));
            }
        }
    }
    Ok(value)
}

/// Resolve the configuration actually read by Controls. Unused product settings
/// are ordinary files, not evidence about this lamp's head delivery.
pub(crate) fn active_pins(
    root: &Path,
    units: &Path,
    carried: &BTreeMap<String, String>,
) -> Result<Option<String>, String> {
    let Some(text) = reader(root, units, carried, CONTROLS)? else {
        return Ok(None);
    };
    let Some(path) = argument(&text, "--pins")? else {
        return Ok(None);
    };
    if !Path::new(path).is_absolute() || Path::new(path).file_name().is_none() {
        return Err("head touch needs an absolute Controls pins filename".into());
    }
    Ok(Some(path.into()))
}

fn bus_root(text: &str, flag: Option<&str>, empty_is_default: bool) -> Result<String, String> {
    let explicit = flag.map(|flag| argument(text, flag)).transpose()?.flatten();
    let environment = unit::environment(text, "LAMP_BUS_ROOT");
    let root = explicit
        .or_else(|| {
            environment
                .as_deref()
                .filter(|root| !empty_is_default || !root.is_empty())
        })
        .unwrap_or("/run/lampos-bus");
    if !Path::new(root).is_absolute() {
        return Err("head touch needs an absolute bus root".into());
    }
    Ok(root.into())
}

fn legacy(root: &Path, units: &Path, carried: &BTreeMap<String, String>) -> Result<bool, String> {
    let mut texts = Vec::new();
    for name in [VOICE, EXPRESSION, CONTROLS] {
        texts.push(
            reader(root, units, carried, name)?
                .ok_or_else(|| format!("head touch needs installed {name}"))?,
        );
    }
    let voice = unit::environment(&texts[0], "LAMP_HEAD_TOUCH").as_deref() == Some("on");
    let expression = flag(&texts[1], "--head-touch");
    let mute = argument(&texts[0], "--controls-mute")?
        .is_some_and(|path| Path::new(path).is_absolute() && Path::new(path).file_name().is_some());
    if voice != expression || (voice && !mute) {
        return Err(
            "head touch needs Voice, its --controls-mute and Expression enabled together".into(),
        );
    }
    let controls = bus_root(&texts[2], Some("--bus-root"), true)?;
    let expression_root = bus_root(&texts[1], Some("--bus-root"), true)?;
    let privacy = unit::environment(&texts[0], "LAMP_PRIVACY_CONTROLS");
    let privacy_matches = !matches!(privacy.as_deref(), Some("on" | "no-slide"))
        || Path::new(&bus_root(&texts[0], None, false)?) == Path::new(&controls);
    let voice_matches = !voice
        || Path::new(&bus_root(&texts[0], Some("--controls-bus-root"), false)?)
            == Path::new(&controls);
    if Path::new(&expression_root) != Path::new(&controls) || !privacy_matches || !voice_matches {
        return Err("head touch needs its enabled readers and Controls on the same bus, including the enabled privacy reader".into());
    }
    Ok(!voice)
}

fn pins(text: &str) -> Result<Option<serde_json::Map<String, serde_json::Value>>, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("head pins cannot be read: {e}"))?;
    let object = value.as_object().ok_or("head pins are not an object")?;
    Ok(object
        .get("head_pads")
        .and_then(|pads| pads.as_array())
        .is_some_and(|pads| !pads.is_empty())
        .then(|| object.clone()))
}

/// Only a build-owned write/record is adapted; custom pins stay custom.
pub(crate) fn project(
    root: &Path,
    units: &Path,
    carried: &BTreeMap<String, String>,
    settings: &[Setting],
    steps: &mut Vec<Step>,
) -> Result<(), String> {
    if active_pins(root, units, carried)?.as_deref() != Some(PINS) {
        return Ok(());
    }
    let Some(setting) = settings
        .iter()
        .find(|setting| setting.path == PINS && !setting.removed)
    else {
        return Ok(());
    };
    let Some(text) = setting.text.as_deref() else {
        return Ok(());
    };
    let Some(mut object) = pins(text)? else {
        return Ok(());
    };
    let wanted = legacy(root, units, carried)?;
    if object
        .get("legacy_head")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
        == wanted
    {
        return Ok(());
    }
    let at = match steps.iter().position(|step| step.setting.path == PINS) {
        Some(at) if matches!(steps[at].act, Act::Write | Act::Record) => at,
        Some(_) => return Ok(()),
        None => {
            // plan omits an already-recorded exact copy of the signed input.
            let Some(current) = read(&root.join(PINS.trim_start_matches('/')))? else {
                return Ok(());
            };
            if Some(manifest::sha256_hex(current.as_bytes())) != setting.sha256 {
                return Ok(());
            }
            steps.push(Step {
                setting: setting.clone(),
                act: Act::Record,
            });
            steps.len() - 1
        }
    };
    object.insert("legacy_head".into(), wanted.into());
    let text = format!(
        "{}\n",
        serde_json::to_string_pretty(&object).map_err(|e| e.to_string())?
    );
    steps[at].setting.text = Some(text.clone());
    steps[at].setting.sha256 = Some(manifest::sha256_hex(text.as_bytes()));
    steps[at].act =
        if read(&root.join(PINS.trim_start_matches('/')))?.as_deref() == Some(text.as_str()) {
            Act::Record
        } else {
            Act::Write
        };
    Ok(())
}

/// Check the result, including settings kept as the lamp's own or by an older
/// installed applier. Called before any service restarts; a mismatch rolls back.
pub(crate) fn verify(
    root: &Path,
    units: &Path,
    carried: &BTreeMap<String, String>,
    steps: &[Step],
) -> Result<(), String> {
    let Some(path) = active_pins(root, units, carried)? else {
        return Ok(());
    };
    let text = match steps.iter().find(|step| step.setting.path == path) {
        Some(step) if step.act == Act::Write => step.setting.text.clone(),
        Some(step) if step.act == Act::Remove => None,
        _ => read(&root.join(path.trim_start_matches('/')))?,
    };
    let Some(text) = text else {
        return Ok(());
    };
    let Some(object) = pins(&text)? else {
        return Ok(());
    };
    let wanted = legacy(root, units, carried)?;
    if object
        .get("legacy_head")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
        != wanted
    {
        return Err("head pins do not match the installed readers; pair the units and pins, and refresh lamp-update-back with install if it predates this check".into());
    }
    Ok(())
}
