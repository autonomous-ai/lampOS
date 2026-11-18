//! What a verified FORMAT 2 package gives the helper, as plain data, and the
//! trait that reads one. Nine's parser (piece 1) implements [`PackageReader`]
//! in the helper's entry (B3): it checks the signature and every file's size
//! and SHA-256 and returns this. Nothing here parses a manifest.
use crate::{facts::AppFacts, name::app_name_ok};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// The most files an app's package holds.
pub const MOST_FILES: usize = 512;
/// The most parts of a path in a package.
pub const MOST_DEPTH: usize = 8;
const MOST_PART: usize = 64;

/// What the manifest said that the helper acts on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppSpec {
    pub name: String,
    pub title: String,
    pub version: String,
    pub min_build: u64,
    pub net: bool,
    pub grants: Vec<String>,
    pub entry: String,
}

impl AppFacts for AppSpec {
    fn name(&self) -> &str {
        &self.name
    }
    fn title(&self) -> &str {
        &self.title
    }
    fn version(&self) -> &str {
        &self.version
    }
    fn min_build(&self) -> u64 {
        self.min_build
    }
    fn net(&self) -> bool {
        self.net
    }
    fn grants(&self) -> &[String] {
        &self.grants
    }
    fn entry(&self) -> &str {
        &self.entry
    }
}

/// A companion skill's folder: its name and its files by path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillFolder {
    pub name: String,
    pub files: BTreeMap<String, Vec<u8>>,
}

/// A package read and verified: the app, its files by path (`src/…`,
/// `assets/…`, and the manifest `lamp-skill.json`), and its companion skills.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppPackage {
    pub app: AppSpec,
    pub files: BTreeMap<String, Vec<u8>>,
    pub skills: Vec<SkillFolder>,
}

pub trait PackageReader {
    /// Reads a package from its bytes, verified: the signature with the
    /// Store's key and every file against the manifest. A format-1 package or
    /// any other thing is an `Err`.
    fn read(&self, bytes: &[u8]) -> Result<AppPackage, String>;
}

/// The SHA-256 of `bytes`, lowercase hex: what the owner's yes is keyed to.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn parts_ok(path: &str) -> Option<Vec<&str>> {
    let parts: Vec<&str> = path.split('/').collect();
    let plain = |part: &&str| {
        !part.is_empty()
            && part.len() <= MOST_PART
            && !part.starts_with('.')
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    };
    (parts.len() <= MOST_DEPTH && parts.iter().all(plain)).then_some(parts)
}

/// A path in an app's own files: the manifest itself, or a file under `src/`
/// or `assets/`. Nothing else, and never a name the helper writes itself.
pub fn app_path_ok(path: &str) -> bool {
    if path == "lamp-skill.json" {
        return true;
    }
    matches!(parts_ok(path), Some(parts) if parts.len() >= 2 && matches!(parts[0], "src" | "assets"))
}

/// A path in a companion skill's folder.
pub fn skill_path_ok(path: &str) -> bool {
    parts_ok(path).is_some()
}

/// Every path of a package, before anything is written.
pub fn paths_ok(package: &AppPackage) -> Result<(), String> {
    let count: usize =
        package.files.len() + package.skills.iter().map(|s| s.files.len()).sum::<usize>();
    if count == 0 || count > MOST_FILES {
        return Err(format!("{count} files: none, or more than {MOST_FILES}"));
    }
    if let Some(path) = package.files.keys().find(|p| !app_path_ok(p)) {
        return Err(format!("not a path an app may have: {path:?}"));
    }
    for skill in &package.skills {
        if !app_name_ok(&skill.name) {
            return Err(format!("not a skill name: {:?}", skill.name));
        }
        if let Some(path) = skill.files.keys().find(|p| !skill_path_ok(p)) {
            return Err(format!("not a path a skill may have: {path:?}"));
        }
    }
    Ok(())
}
