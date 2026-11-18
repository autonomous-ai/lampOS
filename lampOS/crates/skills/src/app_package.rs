//! Format 2: an app's package (#7415 piece 1, `docs/apps.md`).
//!
//! A format-2 package is a tar of ONE folder named for the app, holding
//! `lamp-skill.json` (the manifest below), `lamp-skill.sig` (Ed25519 over the
//! manifest's exact bytes, the Store's key, [`super::package::check_signature`])
//! and every file the manifest lists, each with its size and SHA-256: `src/**`
//! (the app's Python), `assets/**` (WAV clips) and `skills/<name>/**` (the
//! companion skills). Unknown fields are refused at every level, as in
//! format 1; a file not listed, listed twice or missing is refused.
//!
//! This module only READS and CHECKS: [`read_app_package_bytes`] writes
//! nothing and starts nothing. `lamp-apps-manage` installs from what it
//! returns, after the owner's yes. A format-1 reader refuses a format-2
//! package with its own message ("format 2 is not one this lamp reads (1)").
use super::package::{
    Files, MANIFEST, MOST_PACKAGE_BYTES, SIGNATURE, Tar, check_signature, path_ok_within, read_tar,
    sha256_hex, skill_name_ok,
};
use super::version_ok;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// The format this module reads.
pub const FORMAT: u32 = 2;
/// The manifest's `kind`.
pub const KIND: &str = "app";
/// The deepest path in an app package, parts inside its folder.
pub const MOST_DEPTH: usize = 8;
/// The door's grants an app may ask (`lamp_control::protocol`'s plain
/// grants): `net` and `logs` are manifest booleans, never grants, and a
/// service's grant is never an app's own.
pub const GRANTS: [&str; 14] = [
    "body",
    "light",
    "speak",
    "look",
    "hear",
    "sound",
    "skills",
    "air",
    "device",
    "camera",
    "microphone",
    "speaker",
    "notes",
    "speech",
];
/// The longest a phrase or trigger may be, and the fewest letters it holds.
pub const PHRASE_BYTES: usize = 64;
pub const PHRASE_LETTERS: usize = 8;
pub const MOST_PHRASES: usize = 16;
pub const MOST_TRIGGERS: usize = 8;
pub const MOST_SETTINGS: usize = 16;
pub const MOST_ASSETS: usize = 20;
/// An asset's longest, in frames at 24 kHz: 10 s.
pub const ASSET_MOST_FRAMES: u32 = 240_000;

/// `lamp-skill.json` of format 2.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppManifest {
    pub format: u32,
    pub kind: String,
    pub name: String,
    pub title: String,
    pub version: String,
    /// The lamp build's sequence (the number before the `+` in `lamp.build`)
    /// the app needs: a lamp below it refuses to install.
    pub min_build: u64,
    pub runtime: Runtime,
    pub grants: Vec<String>,
    #[serde(default)]
    pub net: bool,
    #[serde(default)]
    pub logs: bool,
    #[serde(default)]
    pub phrases: Vec<String>,
    pub triggers: Vec<String>,
    #[serde(default)]
    pub settings: Vec<Setting>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub assets: Vec<String>,
    pub files: Vec<super::package::Entry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Runtime {
    pub name: String,
    pub entry: String,
}

/// One setting the owner may change; lamp-control checks a value against it
/// before the app sees it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Setting {
    pub key: String,
    #[serde(rename = "type")]
    pub kind: SettingKind,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    pub default: Value,
    /// `text`: the most characters (required, 1 to 200). `number`: the top.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// `number`: the bottom.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// `choice`: 2 to 16.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<Choice>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingKind {
    Text,
    Number,
    Toggle,
    Choice,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    pub value: String,
    pub label: String,
}

/// An app package read and checked.
#[derive(Debug)]
pub struct AppPackage {
    pub manifest: AppManifest,
    /// `lamp-skill.json` as it came (the installed copy is these bytes).
    pub manifest_bytes: Vec<u8>,
    /// The app's own files: `src/**` and `assets/**`, by path.
    pub files: Files,
    /// The companion skills' folders: each skill's files by its path inside
    /// `skills/<name>/`.
    pub skills: BTreeMap<String, Files>,
    pub signed_by: String,
    /// SHA-256 of the whole `.tar`, as the Store's catalogue names it.
    pub sha256: String,
}

/// `^[a-z][a-z0-9-]{0,23}$`.
pub fn app_name_ok(name: &str) -> bool {
    let bytes = name.as_bytes();
    (1..=24).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// `^[a-z0-9_]{1,32}$`.
fn key_ok(key: &str) -> bool {
    (1..=32).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// A choice's value: `[a-z0-9][a-z0-9_-]` up to 32.
fn choice_value_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=32).contains(&bytes.len())
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'))
}

fn plain_text(text: &str, most: usize) -> bool {
    let chars = text.chars().count();
    (1..=most).contains(&chars) && !text.chars().any(char::is_control) && text.trim() == text
}

/// A phrase or trigger: at most 64 characters, at least 8 letters.
fn phrase_ok(phrase: &str) -> bool {
    phrase.len() <= PHRASE_BYTES
        && !phrase.chars().any(char::is_control)
        && phrase.chars().filter(|c| c.is_alphabetic()).count() >= PHRASE_LETTERS
}

fn phrases_ok(what: &str, list: &[String], most: usize) -> Result<(), String> {
    if list.len() > most {
        return Err(format!("{what}: at most {most}"));
    }
    let mut seen = BTreeSet::new();
    for phrase in list {
        if !phrase_ok(phrase) {
            return Err(format!(
                "{what}: {phrase:?} is more than {PHRASE_BYTES} bytes, or has fewer than {PHRASE_LETTERS} letters"
            ));
        }
        if !seen.insert(phrase.to_lowercase()) {
            return Err(format!("{what}: {phrase:?} twice"));
        }
    }
    Ok(())
}

fn setting_ok(setting: &Setting) -> Result<(), String> {
    let at = |why: &str| format!("setting {:?}: {why}", setting.key);
    if !key_ok(&setting.key) {
        return Err(at("a key is 1 to 32 of a-z, 0-9 and _"));
    }
    if !plain_text(&setting.label, 40) {
        return Err(at("a label is 1 to 40 characters on one line"));
    }
    if let Some(hint) = &setting.hint
        && !plain_text(hint, 120)
    {
        return Err(at("a hint is 1 to 120 characters on one line"));
    }
    let (min, max, unit, options) = (
        setting.min,
        setting.max,
        &setting.unit,
        setting.options.as_ref(),
    );
    match setting.kind {
        SettingKind::Text => {
            let most = max.ok_or_else(|| at("a text setting needs max, 1 to 200"))?;
            if most.fract() != 0.0 || !(1.0..=200.0).contains(&most) {
                return Err(at("a text setting's max is a whole number, 1 to 200"));
            }
            if min.is_some() || unit.is_some() || options.is_some() {
                return Err(at("a text setting has no min, unit or options"));
            }
            let Some(default) = setting.default.as_str() else {
                return Err(at("the default of a text setting is text"));
            };
            if default.chars().count() as f64 > most || default.chars().any(char::is_control) {
                return Err(at(
                    "the default is longer than max, or has a control character",
                ));
            }
        }
        SettingKind::Number => {
            let (Some(min), Some(max)) = (min, max) else {
                return Err(at("a number setting needs min and max"));
            };
            if !min.is_finite() || !max.is_finite() || min >= max {
                return Err(at("min is below max, both finite"));
            }
            if options.is_some() {
                return Err(at("a number setting has no options"));
            }
            if let Some(unit) = unit
                && !plain_text(unit, 16)
            {
                return Err(at("a unit is 1 to 16 characters"));
            }
            let Some(default) = setting.default.as_f64() else {
                return Err(at("the default of a number setting is a number"));
            };
            if default < min || default > max {
                return Err(at("the default is outside min and max"));
            }
        }
        SettingKind::Toggle => {
            if min.is_some() || max.is_some() || unit.is_some() || options.is_some() {
                return Err(at("a toggle has only key, label, hint and default"));
            }
            if !setting.default.is_boolean() {
                return Err(at("the default of a toggle is true or false"));
            }
        }
        SettingKind::Choice => {
            if min.is_some() || max.is_some() || unit.is_some() {
                return Err(at("a choice has options, no min, max or unit"));
            }
            let Some(options) = options else {
                return Err(at("a choice needs 2 to 16 options"));
            };
            if !(2..=16).contains(&options.len()) {
                return Err(at("a choice has 2 to 16 options"));
            }
            let mut values = BTreeSet::new();
            for option in options {
                if !choice_value_ok(&option.value) || !plain_text(&option.label, 40) {
                    return Err(at(
                        "an option's value is [a-z0-9][a-z0-9_-] up to 32, its label 1 to 40 characters",
                    ));
                }
                if !values.insert(option.value.as_str()) {
                    return Err(at("an option's value twice"));
                }
            }
            let Some(default) = setting.default.as_str() else {
                return Err(at("the default of a choice is one of its values"));
            };
            if !values.contains(default) {
                return Err(at("the default is not one of the options' values"));
            }
        }
    }
    Ok(())
}

/// The entry's dotted module path: `a.b.c`, each part a Python name.
fn entry_ok(entry: &str) -> bool {
    entry.len() <= 100
        && !entry.is_empty()
        && entry.split('.').all(|part| {
            let bytes = part.as_bytes();
            !bytes.is_empty()
                && (bytes[0].is_ascii_lowercase() || bytes[0] == b'_')
                && bytes
                    .iter()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
        })
}

/// An app's manifest checked on its own, against its own rules (no files).
pub fn check_manifest(manifest: &AppManifest) -> Result<(), String> {
    if manifest.format != FORMAT {
        return Err(format!(
            "{MANIFEST}: format {} is not an app's ({FORMAT})",
            manifest.format
        ));
    }
    if manifest.kind != KIND {
        return Err(format!("{MANIFEST}: kind is {KIND:?} for an app"));
    }
    if !app_name_ok(&manifest.name) {
        return Err(format!(
            "{MANIFEST}: {:?} is not an app's name: 1 to 24 lowercase letters, digits and hyphens, a letter first",
            manifest.name
        ));
    }
    if !plain_text(&manifest.title, 40) {
        return Err(format!(
            "{MANIFEST}: a title is 1 to 40 characters on one line"
        ));
    }
    if !version_ok(&manifest.version) {
        return Err(format!(
            "{MANIFEST}: version {:?} is not MAJOR.MINOR.PATCH",
            manifest.version
        ));
    }
    if manifest.min_build == 0 {
        return Err(format!(
            "{MANIFEST}: min_build is the lamp build's sequence, 1 or more"
        ));
    }
    if manifest.runtime.name != "python3" {
        return Err(format!(
            "{MANIFEST}: runtime.name {:?} is not one this lamp runs (python3)",
            manifest.runtime.name
        ));
    }
    if !entry_ok(&manifest.runtime.entry) {
        return Err(format!(
            "{MANIFEST}: runtime.entry {:?} is not a dotted module path (a.b.c)",
            manifest.runtime.entry
        ));
    }
    let mut grants = BTreeSet::new();
    for grant in &manifest.grants {
        if !GRANTS.contains(&grant.as_str()) {
            return Err(format!(
                "{MANIFEST}: grant {grant:?} is not one of the door's: {}",
                GRANTS.join(", ")
            ));
        }
        if !grants.insert(grant.as_str()) {
            return Err(format!("{MANIFEST}: grant {grant:?} twice"));
        }
    }
    phrases_ok("phrases", &manifest.phrases, MOST_PHRASES)?;
    phrases_ok("triggers", &manifest.triggers, MOST_TRIGGERS)?;
    if (!manifest.phrases.is_empty() || !manifest.triggers.is_empty()) && !grants.contains("hear") {
        return Err(format!(
            "{MANIFEST}: phrases and triggers need the hear grant"
        ));
    }
    if manifest.settings.len() > MOST_SETTINGS {
        return Err(format!("{MANIFEST}: at most {MOST_SETTINGS} settings"));
    }
    let mut keys = BTreeSet::new();
    for setting in &manifest.settings {
        setting_ok(setting).map_err(|why| format!("{MANIFEST}: {why}"))?;
        if !keys.insert(setting.key.as_str()) {
            return Err(format!("{MANIFEST}: setting {:?} twice", setting.key));
        }
    }
    let mut skills = BTreeSet::new();
    for skill in &manifest.skills {
        if !skill_name_ok(skill) {
            return Err(format!("{MANIFEST}: {skill:?} is not a skill's name"));
        }
        if !skills.insert(skill.as_str()) {
            return Err(format!("{MANIFEST}: skill {skill:?} twice"));
        }
    }
    if manifest.assets.len() > MOST_ASSETS {
        return Err(format!("{MANIFEST}: at most {MOST_ASSETS} assets"));
    }
    let mut assets = BTreeSet::new();
    for asset in &manifest.assets {
        if !asset.starts_with("assets/") || !asset.ends_with(".wav") {
            return Err(format!(
                "{MANIFEST}: asset {asset:?} is under assets/ and ends .wav"
            ));
        }
        if !assets.insert(asset.as_str()) {
            return Err(format!("{MANIFEST}: asset {asset:?} twice"));
        }
    }
    Ok(())
}

/// A package's bytes, read and checked as an app's: the signature is one of
/// `keys` over the manifest's exact bytes, checked before the manifest is
/// parsed; then the manifest's own rules, the folder's name, every file's
/// size and SHA-256 and no file but the listed, the entry's module, the
/// companion skills and the assets. Nothing is written anywhere.
pub fn read_app_package_bytes(bytes: &[u8], keys: &[[u8; 32]]) -> Result<AppPackage, String> {
    if bytes.len() as u64 > MOST_PACKAGE_BYTES {
        return Err(format!(
            "more than {MOST_PACKAGE_BYTES} bytes: too big for an app"
        ));
    }
    let Tar {
        manifest,
        signature,
        files,
        folder,
    } = read_tar(bytes, MOST_DEPTH)?;
    let manifest_bytes = manifest.ok_or(format!("no {MANIFEST}: not an app's package"))?;
    let signed_by = check_signature(&manifest_bytes, signature.as_deref(), keys)?;
    let manifest: AppManifest =
        serde_json::from_slice(&manifest_bytes).map_err(|error| format!("{MANIFEST}: {error}"))?;
    check_manifest(&manifest)?;
    if folder.as_deref() != Some(manifest.name.as_str()) {
        return Err(format!(
            "{MANIFEST} names {:?}, and the package's folder is {:?}",
            manifest.name,
            folder.unwrap_or_default()
        ));
    }
    let mut listed = BTreeSet::new();
    for entry in &manifest.files {
        if !listed.insert(entry.path.as_str()) {
            return Err(format!("{MANIFEST} lists {} twice", entry.path));
        }
        if entry.path == MANIFEST || entry.path == SIGNATURE {
            return Err(format!(
                "{MANIFEST} lists {}: it is not one of its files",
                entry.path
            ));
        }
        path_ok_within(&entry.path, MOST_DEPTH)?;
        let top = entry.path.split('/').next().unwrap_or_default();
        if !matches!(top, "src" | "assets" | "skills") || !entry.path.contains('/') {
            return Err(format!(
                "{}: an app's files are under src/, assets/ and skills/",
                entry.path
            ));
        }
        let Some(data) = files.get(&entry.path) else {
            return Err(format!(
                "{} is in {MANIFEST} and not in the package",
                entry.path
            ));
        };
        if data.len() as u64 != entry.size || sha256_hex(data) != entry.sha256 {
            return Err(format!(
                "{}: its size or sha256 is not what {MANIFEST} says: the package was changed or damaged",
                entry.path
            ));
        }
    }
    if let Some(extra) = files.keys().find(|path| !listed.contains(path.as_str())) {
        return Err(format!("{extra} is in the package and not in {MANIFEST}"));
    }
    // The entry's module is there: a.b.c is src/a/b/c.py or src/a/b/c/__init__.py.
    let module = manifest.runtime.entry.replace('.', "/");
    if !files.contains_key(&format!("src/{module}.py"))
        && !files.contains_key(&format!("src/{module}/__init__.py"))
    {
        return Err(format!(
            "runtime.entry {:?} is not in src/: no src/{module}.py or src/{module}/__init__.py",
            manifest.runtime.entry
        ));
    }
    // The assets listed are exactly the WAV files under assets/, each a clip.
    let under: BTreeSet<&str> = files
        .keys()
        .filter(|path| path.starts_with("assets/"))
        .map(String::as_str)
        .collect();
    let named: BTreeSet<&str> = manifest.assets.iter().map(String::as_str).collect();
    if under != named {
        return Err(format!(
            "{MANIFEST}'s assets and the files under assets/ differ: {:?}",
            under
                .symmetric_difference(&named)
                .next()
                .copied()
                .unwrap_or_default()
        ));
    }
    for asset in &manifest.assets {
        lamp_voice::say::decode_sound_with_limit(&files[asset.as_str()], ASSET_MOST_FRAMES)
            .map_err(|why| format!("{asset}: {why}"))?;
    }
    // The companion skills: `skills` names exactly the folders under skills/,
    // each holding a `skill.toml`.
    let mut folders: BTreeMap<String, Files> = BTreeMap::new();
    for (path, data) in &files {
        if let Some(rest) = path.strip_prefix("skills/") {
            let Some((name, inside)) = rest.split_once('/') else {
                return Err(format!(
                    "{path}: a file beside the companion skills' folders"
                ));
            };
            folders
                .entry(name.to_owned())
                .or_default()
                .insert(inside.to_owned(), data.clone());
        }
    }
    let named: BTreeSet<&str> = manifest.skills.iter().map(String::as_str).collect();
    let present: BTreeSet<&str> = folders.keys().map(String::as_str).collect();
    if named != present {
        return Err(format!(
            "{MANIFEST}'s skills and the folders under skills/ differ: {:?}",
            named
                .symmetric_difference(&present)
                .next()
                .copied()
                .unwrap_or_default()
        ));
    }
    for (name, folder) in &folders {
        if !skill_name_ok(name) {
            return Err(format!("skills/{name}: not a skill's name"));
        }
        if !folder.contains_key("skill.toml") {
            return Err(format!(
                "skills/{name}: no skill.toml: not a skill's folder"
            ));
        }
    }
    let kept: Files = files
        .into_iter()
        .filter(|(path, _)| !path.starts_with("skills/"))
        .collect();
    Ok(AppPackage {
        manifest,
        manifest_bytes,
        files: kept,
        skills: folders,
        signed_by,
        sha256: sha256_hex(bytes),
    })
}
