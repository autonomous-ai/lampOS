//! Keep the pins' head delivery paired with the units installed at the switch.
//! Settings remain signed inputs; this local projection changes only the
//! compatibility bit, and the ordinary apply journal records the actual bytes.
use crate::{
    apply::{Act, Step},
    manifest::{self, Setting},
    unit,
};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read},
    os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

pub(crate) const PINS: &str = "/etc/lampOS/controls/controls-pins.json";
const CONTROLS: &str = "lamp-controls.service";
const VOICE: &str = "lamp-voice.service";
const EXPRESSION: &str = "lamp-expression.service";
const BODY: &str = "lamp-body.service";
const BODY_ENV: &str = "/etc/lampOS/body/lamp.env";

/// Read dependencies of the existing head-pair policy. A closed implementation
/// must distinguish a captured absence from an uncaptured key. This interface
/// supplies no authentication, exclusion, freshness or write authority.
pub(crate) trait HeadInputs {
    fn text(&mut self, path: &Path) -> Result<Option<String>, String>;
    fn entries(&mut self, directory: &Path) -> Result<Option<ConfEntries>, String>;
}

/// Owned and lazy: policy may reject an early file without consuming a later
/// directory error. Entries include ignored names so a collector can bound all
/// enumeration work before filtering. No iterator borrows its input provider.
pub(crate) type ConfEntries = Box<dyn Iterator<Item = Result<PathBuf, String>>>;

struct OrdinaryInputs;

impl HeadInputs for OrdinaryInputs {
    fn text(&mut self, path: &Path) -> Result<Option<String>, String> {
        read(path)
    }

    fn entries(&mut self, directory: &Path) -> Result<Option<ConfEntries>, String> {
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("head touch overrides cannot be read: {error}")),
        };
        Ok(Some(Box::new(entries.map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|error| error.to_string())
        }))))
    }
}

fn read(path: &Path) -> Result<Option<String>, String> {
    let failed = |error: io::Error| format!("{}: {error}", path.display());
    let flags = rustix::fs::OFlags::NONBLOCK | rustix::fs::OFlags::NOCTTY;
    let mut file = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(flags.bits() as i32)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(failed(error)),
    };
    // Inspect the descriptor we will read. A FIFO must not wait for a writer;
    // regular symlinks and systemd's actual null-device masks remain valid.
    let meta = file.metadata().map_err(failed)?;
    if !meta.is_file() {
        if is_null_device(&meta) {
            return Ok(Some(String::new()));
        }
        return Err(format!(
            "{}: head touch needs a regular file or /dev/null mask",
            path.display()
        ));
    }
    let mut text = String::new();
    file.read_to_string(&mut text).map_err(failed)?;
    Ok(Some(text))
}

fn is_null_device(meta: &fs::Metadata) -> bool {
    meta.file_type().is_char_device()
        && fs::metadata("/dev/null")
            .is_ok_and(|null| null.file_type().is_char_device() && null.rdev() == meta.rdev())
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
fn no_mode_overrides(inputs: &mut dyn HeadInputs, root: &Path, name: &str) -> Result<(), String> {
    for base in [
        "etc/systemd/system",
        "run/systemd/system",
        "usr/local/lib/systemd/system",
        "usr/lib/systemd/system",
        "lib/systemd/system",
    ] {
        for folder in [
            "service.d".to_owned(),
            "lamp-.service.d".to_owned(),
            format!("{name}.d"),
        ] {
            let entries = match inputs.entries(&root.join(base).join(folder))? {
                Some(entries) => entries,
                None => continue,
            };
            for entry in entries {
                let path = entry?;
                if path.extension().is_none_or(|ext| ext != "conf") {
                    continue;
                }
                let text = inputs.text(&path)?.unwrap_or_default();
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
    inputs: &mut dyn HeadInputs,
    root: &Path,
    units: &Path,
    carried: &BTreeMap<String, String>,
    name: &str,
) -> Result<Option<String>, String> {
    no_mode_overrides(inputs, root, name)?;
    let text = match carried.get(name) {
        Some(text) => text.clone(),
        None => match inputs.text(&units.join(name))? {
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
    active_pins_with(&mut OrdinaryInputs, root, units, carried)
}

fn active_pins_with(
    inputs: &mut dyn HeadInputs,
    root: &Path,
    units: &Path,
    carried: &BTreeMap<String, String>,
) -> Result<Option<String>, String> {
    let Some(text) = reader(inputs, root, units, carried, CONTROLS)? else {
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

// Body deliberately reads its factory calibration and expands the hostname and
// poses in ExecStart. Admit only those known exceptions to the literal reader.
// A calibration file can override Environment= too, so inspect its resulting
// bytes rather than assuming it contains poses. These per-lamp calibration
// paths are not allowed manifest settings; install and update preserve them.
fn body_reader(
    inputs: &mut dyn HeadInputs,
    root: &Path,
    units: &Path,
    carried: &BTreeMap<String, String>,
) -> Result<String, String> {
    let text = match carried.get(BODY) {
        Some(text) => text.clone(),
        None => inputs
            .text(&units.join(BODY))?
            .ok_or("head touch needs installed lamp-body.service")?,
    };
    let mut normalized = Vec::new();
    let mut commands = 0;
    for line in service(&text).lines() {
        let calibration_path = line
            .strip_prefix("EnvironmentFile=")
            .map(|path| path.strip_prefix('-').unwrap_or(path))
            .filter(|path| matches!(*path, BODY_ENV | "/var/lib/lampos-body/measured.env"));
        if let Some(path) = calibration_path {
            let calibration = inputs.text(&root.join(path.trim_start_matches('/')))?;
            // A new lamp may not have measured poses yet. The required file
            // then prevents Body from starting, as it did before this check.
            for line in calibration.as_deref().unwrap_or_default().lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with(['#', ';']) {
                    continue;
                }
                let poses = line
                    .strip_prefix("BODY_POSES=\"")
                    .and_then(|value| value.strip_suffix('"'));
                if poses.is_none_or(|value| value.contains(['"', '\\'])) {
                    return Err("head touch needs Body calibration to contain only literal BODY_POSES; its environment could change the sleep reader".into());
                }
            }
            continue;
        }
        if let Some(command) = line.strip_prefix("ExecStart=") {
            commands += 1;
            let command = command
                .split_whitespace()
                .map(|word| match word {
                    "%H" => "lamp-hostname",
                    "$BODY_POSES" => "",
                    _ => word,
                })
                .collect::<Vec<_>>()
                .join(" ");
            normalized.push(format!("ExecStart={command}"));
        } else {
            normalized.push(line.to_owned());
        }
    }
    if commands != 1 {
        return Err("head touch needs one literal Body ExecStart".into());
    }
    let normalized = BTreeMap::from([(
        BODY.to_owned(),
        format!("[Service]\n{}", normalized.join("\n")),
    )]);
    reader(inputs, root, units, &normalized, BODY)?
        .ok_or("head touch needs installed lamp-body.service".into())
}

fn legacy(
    inputs: &mut dyn HeadInputs,
    root: &Path,
    units: &Path,
    carried: &BTreeMap<String, String>,
) -> Result<bool, String> {
    let mut texts = Vec::new();
    for name in [VOICE, EXPRESSION, CONTROLS] {
        texts.push(
            reader(inputs, root, units, carried, name)?
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
    if voice {
        let body = body_reader(inputs, root, units, carried)?;
        if unit::environment(&body, "LAMP_HEAD_TOUCH").as_deref() != Some("on")
            || Path::new(&bus_root(&body, None, true)?) != Path::new(&controls)
        {
            return Err("head touch needs Body's sleep reader enabled on the same bus as Voice and Controls".into());
        }
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

/// Installation replaces the base product units but preserves local drop-ins.
/// Check their resulting reader mode before install changes users, links, units
/// or pins. The same conservative override rules as an update round apply.
pub(crate) fn preflight_install(
    root: &Path,
    units: &Path,
    product: &[(&str, &str)],
    settings: &[(&str, &str)],
) -> Result<(), String> {
    let text = settings
        .iter()
        .find_map(|(path, text)| (*path == PINS).then_some(*text))
        .ok_or("install has no product Controls pins")?;
    let Some(object) = pins(text)? else {
        return Ok(());
    };
    let carried = product
        .iter()
        .map(|(name, text)| ((*name).to_owned(), (*text).to_owned()))
        .collect();
    let wanted = legacy(&mut OrdinaryInputs, root, units, &carried)?;
    let configured = object
        .get("legacy_head")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    if configured != wanted {
        return Err("install head pins do not match the resulting product readers".into());
    }
    Ok(())
}

/// Only a build-owned write/record is adapted; custom pins stay custom.
pub(crate) fn project(
    root: &Path,
    units: &Path,
    carried: &BTreeMap<String, String>,
    settings: &[Setting],
    steps: &mut Vec<Step>,
) -> Result<(), String> {
    project_with(&mut OrdinaryInputs, root, units, carried, settings, steps)
}

pub(crate) fn project_with(
    inputs: &mut dyn HeadInputs,
    root: &Path,
    units: &Path,
    carried: &BTreeMap<String, String>,
    settings: &[Setting],
    steps: &mut Vec<Step>,
) -> Result<(), String> {
    if active_pins_with(inputs, root, units, carried)?.as_deref() != Some(PINS) {
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
    let wanted = legacy(inputs, root, units, carried)?;
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
            let Some(current) = inputs.text(&root.join(PINS.trim_start_matches('/')))? else {
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
    steps[at].act = if inputs
        .text(&root.join(PINS.trim_start_matches('/')))?
        .as_deref()
        == Some(text.as_str())
    {
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
    verify_with(&mut OrdinaryInputs, root, units, carried, steps)
}

pub(crate) fn verify_with(
    inputs: &mut dyn HeadInputs,
    root: &Path,
    units: &Path,
    carried: &BTreeMap<String, String>,
    steps: &[Step],
) -> Result<(), String> {
    let Some(path) = active_pins_with(inputs, root, units, carried)? else {
        return Ok(());
    };
    let text = match steps.iter().find(|step| step.setting.path == path) {
        Some(step) if step.act == Act::Write => step.setting.text.clone(),
        Some(step) if step.act == Act::Remove => None,
        _ => inputs.text(&root.join(path.trim_start_matches('/')))?,
    };
    let Some(text) = text else {
        return Ok(());
    };
    let Some(object) = pins(&text)? else {
        return Ok(());
    };
    let wanted = legacy(inputs, root, units, carried)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_masks_do_not_admit_other_character_devices() {
        // Inspect metadata only: an erroneous reader of /dev/zero could
        // allocate without bound before a watchdog has time to stop it.
        let null = fs::metadata("/dev/null").unwrap();
        let zero = fs::metadata("/dev/zero").unwrap();
        assert!(null.file_type().is_char_device());
        assert!(zero.file_type().is_char_device());
        assert_ne!(null.rdev(), zero.rdev());
        assert!(is_null_device(&null));
        assert!(!is_null_device(&zero));
    }
}

#[cfg(test)]
mod input_tests;
