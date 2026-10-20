//! Core and installable skills (`docs/skills.md`, "Core and installable
//! skills"). A core skill comes with the lamp, as a phone's own apps do:
//! `lamp-update install` writes it and nothing here removes or replaces it.
//! An installable one is its owner's to put on and take off a running lamp,
//! from a folder or from a package, the `.tar` the Skills Store will carry.
//! Nothing else is restarted: the running `lamp-skills` looks at its folder
//! every two seconds and loads what changed.
//!
//! A package is a tar of one skill's folder and a manifest, `lamp-skill.json`,
//! naming the format, the skill, its version and every file with its size
//! and SHA-256, each checked before anything is installed. A signature has
//! a reserved place, `lamp-skill.sig`, and is NOT checked: signing is in
//! the backlog. There is no fetch from a network: a package is a local file.
use super::{Skill, SkillKind, load, load_each, plain_name, version_ok};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    io::{BufRead, Read},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// The package format this program writes and reads.
pub const FORMAT: u32 = 1;
/// The manifest's name at the top of a package.
pub const MANIFEST: &str = "lamp-skill.json";
/// Reserved for the Store's signature over `lamp-skill.json`'s exact bytes.
/// Signing is in the backlog: a package may carry this file, and nothing
/// checks it yet, which every install says.
pub const SIGNATURE: &str = "lamp-skill.sig";
/// The most files a skill may have, its tests among them.
pub const MOST_FILES: usize = 512;
/// The most bytes a skill's files may come to (the largest of the lamp's
/// own, Stuffy air with its recordings, is about 1.4 MB).
pub const MOST_BYTES: u64 = 16 * 1024 * 1024;
/// A package's own size at most: its files and the tar's headers.
const MOST_PACKAGE_BYTES: u64 = MOST_BYTES + 2 * 1024 * 1024;
/// The most parts a file's path in a skill may have (`tests/CASE/FILE`).
const MOST_DEPTH: usize = 4;
/// The most functions a session is given, the voice's own and the actions
/// part's told first, then the skills' in their folders' order
/// (`docs/tools.md`): past it, the last are left out.
pub const SESSION_FUNCTIONS: usize = 32;

/// `lamp-skill.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub format: u32,
    pub name: String,
    pub version: String,
    pub files: Vec<Entry>,
}

/// One file of a skill, as its manifest lists it: its path in the skill's
/// folder, `/` between its parts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

/// A skill's files by their path in its folder, `/` between the parts.
pub type Files = BTreeMap<String, Vec<u8>>;

/// A package read and checked: its manifest, the skill's files, each as the
/// manifest says, and the signature it carries, if any, unchecked.
#[derive(Debug)]
pub struct Package {
    pub manifest: Manifest,
    pub files: Files,
    pub signature: Option<Vec<u8>>,
}

/// Lowercase hex of `bytes`' SHA-256.
pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// A skill's name: 1 to 64 lowercase letters, digits and hyphens.
pub fn skill_name_ok(name: &str) -> bool {
    plain_name(name) && !name.contains('_')
}

/// One part of a path in a skill: letters, digits, `-`, `_` and `.`, not
/// beginning with a dot (hidden, as a Mac's `._NAME`), at most 100 bytes.
fn part_ok(part: &str) -> bool {
    (1..=100).contains(&part.len())
        && !part.starts_with('.')
        && part
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// A file's path in a skill: 1 to `MOST_DEPTH` good parts.
fn path_ok(path: &str) -> Result<(), String> {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() > MOST_DEPTH || !parts.iter().all(|part| part_ok(part)) {
        return Err(format!(
            "{path:?}: a skill's paths are at most {MOST_DEPTH} parts of letters, digits, -, _ and ., \
             none beginning with a dot"
        ));
    }
    Ok(())
}

/// The plain file at `path`, opened without following a link at its end and
/// checked as opened: a regular file, and the one its folder listed
/// (`listed`, by its device and inode). A file swapped for a link or for
/// another file between the listing and the open is refused, never read:
/// run as root on a folder another user can write, a link swapped in would
/// otherwise copy a file only root may read into the skill, made 0644.
/// Opened without blocking too, so a fifo swapped in does not hang it.
fn open_plain(path: &Path, listed: &fs::Metadata) -> Result<fs::File, String> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|error| {
            if error.raw_os_error() == Some(libc::ELOOP) {
                "a link: a skill is plain files and folders".to_owned()
            } else {
                error.to_string()
            }
        })?;
    let opened = file.metadata().map_err(|error| error.to_string())?;
    if !opened.is_file() {
        return Err("not a plain file".into());
    }
    if (opened.dev(), opened.ino()) != (listed.dev(), listed.ino()) {
        return Err("changed while it was read: not the file its folder listed".into());
    }
    Ok(file)
}

/// Every file of the skill folder `dir` but hidden ones (their name begins
/// with a dot, as the loader passes them over). A link, or anything but a
/// plain file or folder, refuses it: a skill is plain files. Each file is
/// read as [`open_plain`] opens it.
pub fn read_folder(dir: &Path) -> Result<Files, String> {
    let mut files = Files::new();
    let mut total = 0u64;
    let mut folders = vec![(dir.to_path_buf(), String::new())];
    while let Some((folder, prefix)) = folders.pop() {
        let entries =
            fs::read_dir(&folder).map_err(|error| format!("{}: {error}", folder.display()))?;
        for entry in entries {
            let entry = entry.map_err(|error| format!("{}: {error}", folder.display()))?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                return Err(format!(
                    "{}: a name that is not UTF-8",
                    entry.path().display()
                ));
            };
            if name.starts_with('.') {
                continue;
            }
            let path = if prefix.is_empty() {
                name.to_owned()
            } else {
                format!("{prefix}/{name}")
            };
            let meta =
                fs::symlink_metadata(entry.path()).map_err(|error| format!("{path}: {error}"))?;
            if meta.file_type().is_symlink() {
                return Err(format!(
                    "{path}: a link: a skill is plain files and folders"
                ));
            }
            if meta.is_dir() {
                path_ok(&path)?;
                folders.push((entry.path(), path));
                continue;
            }
            if !meta.is_file() {
                return Err(format!("{path}: not a plain file"));
            }
            path_ok(&path)?;
            let too_big = || {
                format!("more than {MOST_FILES} files or {MOST_BYTES} bytes: too big for a skill")
            };
            if total + meta.len() > MOST_BYTES || files.len() >= MOST_FILES {
                return Err(too_big());
            }
            let mut bytes = Vec::new();
            open_plain(&entry.path(), &meta)
                .and_then(|file| {
                    file.take(MOST_BYTES - total + 1)
                        .read_to_end(&mut bytes)
                        .map_err(|error| error.to_string())
                })
                .map_err(|why| format!("{path}: {why}"))?;
            // What was read, which may be more than was listed.
            total += bytes.len() as u64;
            if total > MOST_BYTES {
                return Err(too_big());
            }
            files.insert(path, bytes);
        }
    }
    Ok(files)
}

/// One plain file in a package, written the same way every time (no time,
/// no owner), so the same skill packs to the same bytes.
fn append(builder: &mut tar::Builder<Vec<u8>>, path: &str, bytes: &[u8]) -> Result<(), String> {
    let mut header = tar::Header::new_ustar();
    header.set_entry_type(tar::EntryType::Regular);
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_mtime(0);
    header.set_uid(0);
    header.set_gid(0);
    builder
        .append_data(&mut header, path, bytes)
        .map_err(|error| format!("{path}: {error}"))
}

/// Packs the installable skill folder `dir` as `NAME-VERSION.tar` in `out`,
/// for the Store: the skill must load, say its version, and be installable
/// (a core skill comes with the lamp, never as a package). The package is
/// read back and checked as an install checks it.
pub fn pack(dir: &Path, out: &Path) -> Result<PathBuf, String> {
    let skill = load(dir).map_err(|why| format!("not packed: {why}"))?;
    if skill.kind == SkillKind::Core {
        return Err(format!(
            "not packed: {} is a core skill: it comes with the lamp (lamp-update install), never as a package",
            skill.name
        ));
    }
    let Some(version) = skill.version.clone() else {
        return Err(format!(
            "not packed: {}'s skill.toml says no version: a package carries one, as version = \"1.0.0\"",
            skill.name
        ));
    };
    let files = read_folder(dir)?;
    let manifest = Manifest {
        format: FORMAT,
        name: skill.name.clone(),
        version: version.clone(),
        files: files
            .iter()
            .map(|(path, bytes)| Entry {
                path: path.clone(),
                size: bytes.len() as u64,
                sha256: sha256_hex(bytes),
            })
            .collect(),
    };
    let mut json = serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?;
    json.push(b'\n');
    let mut builder = tar::Builder::new(Vec::new());
    append(&mut builder, MANIFEST, &json)?;
    for (path, bytes) in &files {
        append(&mut builder, &format!("{}/{path}", skill.name), bytes)?;
    }
    let bytes = builder.into_inner().map_err(|error| error.to_string())?;
    fs::create_dir_all(out).map_err(|error| format!("{}: {error}", out.display()))?;
    let path = out.join(format!("{}-{version}.tar", skill.name));
    // Whole or not at all.
    let part = out.join(format!(".{}-{version}.tar.part", skill.name));
    fs::write(&part, &bytes)
        .and_then(|()| fs::rename(&part, &path))
        .map_err(|error| format!("{}: {error}", path.display()))?;
    read_package(&path)
        .map_err(|why| format!("{}: packed, then not read back: {why}", path.display()))?;
    Ok(path)
}

/// The package at `path`, read and checked: a plain tar of a manifest, an
/// optional signature (kept, not checked) and one skill's folder of plain
/// files, every one of them in the manifest with its size and SHA-256, and
/// no other.
pub fn read_package(path: &Path) -> Result<Package, String> {
    let meta = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !meta.is_file() {
        return Err("not a plain file".into());
    }
    if meta.len() > MOST_PACKAGE_BYTES {
        return Err(format!(
            "more than {MOST_PACKAGE_BYTES} bytes: too big for a skill"
        ));
    }
    let mut bytes = Vec::new();
    open_plain(path, &meta).and_then(|file| {
        file.take(MOST_PACKAGE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())
    })?;
    if bytes.len() as u64 > MOST_PACKAGE_BYTES {
        return Err(format!(
            "more than {MOST_PACKAGE_BYTES} bytes: too big for a skill"
        ));
    }
    let mut archive = tar::Archive::new(&bytes[..]);
    let (mut manifest, mut signature) = (None, None);
    let mut files = Files::new();
    let mut folder: Option<String> = None;
    let mut total = 0u64;
    let entries = archive
        .entries()
        .map_err(|error| format!("not a tar: {error}"))?;
    for entry in entries {
        let mut entry = entry.map_err(|error| format!("not a tar: {error}"))?;
        let name = String::from_utf8(entry.path_bytes().into_owned())
            .map_err(|_| "a path that is not UTF-8".to_owned())?;
        let kind = entry.header().entry_type();
        let path = name.trim_end_matches('/').to_owned();
        // The skill's folder, and only it, before the first `/`; its path
        // in the folder after it. A file with no `/` is beside the folder.
        let mut within = |path: &str, folder_entry: bool| -> Result<Option<String>, String> {
            let (top, rest) = match path.split_once('/') {
                Some((top, rest)) => (top, Some(rest)),
                None if folder_entry => (path, None),
                None => return Ok(None),
            };
            if folder.as_deref().is_some_and(|folder| folder != top) {
                return Err(format!(
                    "{path}: a second folder beside {}: a package holds one skill",
                    folder.as_deref().unwrap_or_default()
                ));
            }
            folder = Some(top.to_owned());
            match rest {
                Some(rest) => path_ok(rest).map(|()| Some(rest.to_owned())),
                None => Ok(None),
            }
        };
        if kind.is_dir() {
            within(&path, true)?;
            continue;
        }
        if kind != tar::EntryType::Regular {
            return Err(format!(
                "{path}: a link, a device or another entry that is not a plain file: a package holds plain files"
            ));
        }
        let size = entry.size();
        total += size;
        if total > MOST_BYTES || files.len() >= MOST_FILES {
            return Err(format!(
                "more than {MOST_FILES} files or {MOST_BYTES} bytes: too big for a skill"
            ));
        }
        let mut data = Vec::new();
        (&mut entry)
            .take(size)
            .read_to_end(&mut data)
            .map_err(|error| format!("{path}: {error}"))?;
        if data.len() as u64 != size {
            return Err(format!("{path}: cut short"));
        }
        let kept = match path.as_str() {
            MANIFEST => manifest.replace(data).is_some(),
            SIGNATURE => signature.replace(data).is_some(),
            _ => match within(&path, false)? {
                Some(rest) => files.insert(rest, data).is_some(),
                None => {
                    return Err(format!(
                        "{path}: a file beside the skill's folder that is neither {MANIFEST} nor {SIGNATURE}"
                    ));
                }
            },
        };
        if kept {
            return Err(format!("{path}: twice in the package"));
        }
    }
    let manifest = manifest.ok_or(format!("no {MANIFEST}: not a skill's package"))?;
    let manifest: Manifest =
        serde_json::from_slice(&manifest).map_err(|error| format!("{MANIFEST}: {error}"))?;
    if manifest.format != FORMAT {
        return Err(format!(
            "{MANIFEST}: format {} is not one this lamp reads ({FORMAT})",
            manifest.format
        ));
    }
    if !skill_name_ok(&manifest.name) {
        return Err(format!(
            "{MANIFEST}: {:?} is not a skill's name: 1 to 64 lowercase letters, digits and hyphens",
            manifest.name
        ));
    }
    if folder.as_deref() != Some(manifest.name.as_str()) {
        return Err(format!(
            "{MANIFEST} names {:?}, and the package's folder is {:?}",
            manifest.name,
            folder.unwrap_or_default()
        ));
    }
    if !version_ok(&manifest.version) {
        return Err(format!(
            "{MANIFEST}: version {:?} is not MAJOR.MINOR.PATCH",
            manifest.version
        ));
    }
    let mut listed = BTreeSet::new();
    for entry in &manifest.files {
        if !listed.insert(entry.path.as_str()) {
            return Err(format!("{MANIFEST} lists {} twice", entry.path));
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
    Ok(Package {
        manifest,
        files,
        signature,
    })
}

/// The kind a skill folder's `skill.toml` says, read on its own, so that a
/// folder that no longer loads is still known for what it is: installable
/// when it says none or cannot be read.
fn kind_of(dir: &Path) -> SkillKind {
    toml_of(dir)
        .and_then(|toml| SkillKind::read(toml.get("kind").and_then(toml::Value::as_str)).ok())
        .unwrap_or_default()
}

fn version_of(dir: &Path) -> Option<String> {
    toml_of(dir)?
        .get("version")
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
}

fn toml_of(dir: &Path) -> Option<toml::Table> {
    fs::read_to_string(dir.join("skill.toml"))
        .ok()?
        .parse::<toml::Table>()
        .ok()
}

/// The folder and its files open to every user (the parts run as `lampos`)
/// and written only by their owner, whatever the umask of who installed it.
fn open_to_parts(dir: &Path) -> io::Result<()> {
    fs::set_permissions(dir, fs::Permissions::from_mode(0o755))?;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            open_to_parts(&entry.path())?;
        } else {
            fs::set_permissions(entry.path(), fs::Permissions::from_mode(0o644))?;
        }
    }
    Ok(())
}

/// A folder of `skills` the running program passes over (its name begins
/// with a dot) for this install's own use.
fn hidden_place(skills: &Path, what: &str, name: &str) -> PathBuf {
    skills.join(format!(".{what}-{name}-{}", std::process::id()))
}

/// The skills folder held by this install or remove for its whole run (an
/// advisory lock on the folder itself, given back when the run ends, killed
/// or not), so two never interleave: a staged or set-aside folder another
/// run finds is a killed run's, never one under way, and what one checks
/// of the folder is still so when it moves a skill in.
fn locked(skills: &Path) -> Result<fs::File, String> {
    let folder =
        fs::File::open(skills).map_err(|error| format!("{}: {error}", skills.display()))?;
    folder
        .lock()
        .map_err(|error| format!("{}: not locked: {error}", skills.display()))?;
    Ok(folder)
}

/// [`locked`], but waiting at most `wait` for another run to give the
/// folder back: then [`BUSY`]. The manage helper waits this way, so it never
/// goes on with a remove after its asker has stopped waiting for it.
fn locked_within(skills: &Path, wait: Duration) -> Result<fs::File, String> {
    let folder =
        fs::File::open(skills).map_err(|error| format!("{}: {error}", skills.display()))?;
    let until = Instant::now() + wait;
    loop {
        match folder.try_lock() {
            Ok(()) => return Ok(folder),
            Err(fs::TryLockError::WouldBlock) if Instant::now() < until => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(fs::TryLockError::WouldBlock) => {
                eprintln!(
                    "lamp-skills: {}: held by another run for {} ms: nothing done",
                    skills.display(),
                    wait.as_millis()
                );
                return Err(BUSY.to_owned());
            }
            Err(fs::TryLockError::Error(error)) => {
                return Err(format!("{}: not locked: {error}", skills.display()));
            }
        }
    }
}

/// Why the manage helper did nothing when another run held the skills
/// folder past [`ASKED_LOCK_WAIT`]: the words `lamp-cloud` already gives the
/// app when the helper does not answer.
pub const BUSY: &str = "the lamp's skills can't be changed now";

/// The longest the manage helper waits for the skills folder's lock, under
/// `lamp-cloud`'s 5 s wait for its answer, so a remove is done, or refused,
/// while `lamp-cloud` still waits for it, never after.
pub const ASKED_LOCK_WAIT: Duration = Duration::from_secs(4);

/// Whether `name` is what [`hidden_place`] names: `.install-NAME-PID` or
/// `.removed-NAME-PID`, a skill's name and a number, and nothing else.
fn left_by_a_run(name: &str) -> bool {
    [".install-", ".removed-"].iter().any(|what| {
        name.strip_prefix(what)
            .and_then(|rest| rest.rsplit_once('-'))
            .is_some_and(|(skill, pid)| {
                skill_name_ok(skill)
                    && (1..=10).contains(&pid.len())
                    && pid.bytes().all(|byte| byte.is_ascii_digit())
            })
    })
}

/// Deletes what a killed install or remove left in `skills` (called with
/// the folder [`locked`], so nothing under way), and names it: a folder
/// directly in `skills` whose name [`left_by_a_run`]. A link, a file, or a
/// folder of any other name is left as it is.
fn swept(skills: &Path) -> Vec<String> {
    let mut swept: Vec<String> = fs::read_dir(skills)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| left_by_a_run(name))
        .filter(|name| {
            let place = skills.join(name);
            fs::symlink_metadata(&place).is_ok_and(|meta| meta.is_dir())
                && fs::remove_dir_all(&place).is_ok()
        })
        .collect();
    swept.sort();
    swept
}

/// The skill in `skills`, other than `name`, whose tool is `tool`, and its
/// kind, by each folder's `skill.toml` alone: a skill that does not load
/// just now keeps its function's name all the same.
fn tool_holder(skills: &Path, name: &str, tool: &str) -> Option<(String, SkillKind)> {
    let mut folders: Vec<(String, PathBuf)> = fs::read_dir(skills)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| Some((entry.file_name().into_string().ok()?, entry.path())))
        .filter(|(folder, path)| !folder.starts_with('.') && folder != name && path.is_dir())
        .collect();
    folders.sort();
    folders.into_iter().find_map(|(folder, path)| {
        let toml = toml_of(&path)?;
        (toml.get("tool").and_then(toml::Value::as_str) == Some(tool))
            .then(|| (folder, kind_of(&path)))
    })
}

/// What the enabled `skills` give the model: their functions, and what a
/// session does past its room (`docs/tools.md`). Inventory may include
/// disabled skills that still reserve a name but offer no function.
fn functions_said(skills: &[Skill]) -> String {
    // A skill's `[stop]` is one function more (#1685).
    let tools: Vec<&str> = skills
        .iter()
        .filter(|skill| skill.enabled)
        .flat_map(|skill| {
            skill
                .tool
                .as_deref()
                .into_iter()
                .chain(skill.stop.as_ref().map(|stop| stop.tool.as_str()))
        })
        .collect();
    if tools.is_empty() {
        return "The skills here give the model no function.".into();
    }
    format!(
        "The skills here give the model {} function{} ({}). A session takes {SESSION_FUNCTIONS}, \
         the voice's own and the actions part's first (30 on a product lamp), and past \
         {SESSION_FUNCTIONS} the last are dropped unless the voice runs with LAMP_USE_SKILL=on \
         (every skill through one function, use_skill).",
        tools.len(),
        if tools.len() == 1 { "" } else { "s" },
        tools.join(", ")
    )
}

/// Installs the skill at `from`, a folder or a package (`.tar`), in the
/// skills folder `skills`, and says what was done. It is put together
/// beside the live skills where the running program does not look, loaded
/// there with every check a load makes (its permissions among them), and
/// only then moved in whole: the running program never sees half a skill,
/// and one that does not load is never live. A core skill is refused, and
/// so is one whose name a core skill on the lamp has, or whose tool's name
/// another skill there has. What a killed install or remove left is
/// deleted first ([`swept`]).
pub fn install(skills: &Path, from: &Path) -> Result<String, String> {
    if !fs::metadata(skills).is_ok_and(|meta| meta.is_dir()) {
        return Err(format!("no skills folder at {}", skills.display()));
    }
    let _held = locked(skills)?;
    let swept = swept(skills);
    let meta = fs::metadata(from).map_err(|error| format!("{}: {error}", from.display()))?;
    let (name, files, package) = if meta.is_dir() {
        let from =
            fs::canonicalize(from).map_err(|error| format!("{}: {error}", from.display()))?;
        let name = from
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        (name, read_folder(&from)?, None)
    } else {
        let mut package = read_package(from)
            .map_err(|why| format!("not installed: {}: {why}", from.display()))?;
        let files = std::mem::take(&mut package.files);
        (package.manifest.name.clone(), files, Some(package))
    };
    if !skill_name_ok(&name) {
        return Err(format!(
            "not installed: {name:?} is not a skill's name: 1 to 64 lowercase letters, digits and hyphens"
        ));
    }
    let staged = hidden_place(skills, "install", &name);
    let made = (|| -> Result<String, String> {
        let failed = |error: io::Error| format!("not installed: {}: {error}", staged.display());
        if staged.exists() {
            fs::remove_dir_all(&staged).map_err(failed)?;
        }
        let folder = staged.join(&name);
        for (path, bytes) in &files {
            let at = folder.join(path);
            if let Some(parent) = at.parent() {
                fs::create_dir_all(parent).map_err(failed)?;
            }
            fs::write(&at, bytes).map_err(failed)?;
        }
        fs::create_dir_all(&folder).map_err(failed)?;
        open_to_parts(&folder).map_err(failed)?;
        moved_in(skills, &name, &folder, files.len(), package.as_ref())
    })();
    let _ = fs::remove_dir_all(&staged);
    if swept.is_empty() {
        made
    } else {
        made.map(|said| {
            format!(
                "{said} Deleted what a killed install or remove left: {}.",
                swept.join(", ")
            )
        })
    }
}

/// The skill put together in `folder` loaded, checked against what it
/// replaces, and moved into `skills`.
fn moved_in(
    skills: &Path,
    name: &str,
    folder: &Path,
    count: usize,
    package: Option<&Package>,
) -> Result<String, String> {
    let skill =
        load(folder).map_err(|why| format!("not installed: {name} does not load: {why}"))?;
    if skill.kind == SkillKind::Core {
        return Err(format!(
            "not installed: {name} says kind = \"core\": a core skill comes with the lamp (lamp-update install), \
             never from a folder or a package"
        ));
    }
    if let Some(package) = package
        && skill.version.as_deref() != Some(package.manifest.version.as_str())
    {
        return Err(format!(
            "not installed: {MANIFEST} says version {}, and its skill.toml {}",
            package.manifest.version,
            skill.version.as_deref().unwrap_or("none")
        ));
    }
    let target = skills.join(name);
    let replaced = match fs::symlink_metadata(&target) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("not installed: {}: {error}", target.display())),
        Ok(meta) if !meta.is_dir() => {
            return Err(format!(
                "not installed: {} is there and is not a skill's folder: left as it is",
                target.display()
            ));
        }
        Ok(_) if kind_of(&target) == SkillKind::Core => {
            return Err(format!(
                "not installed: {name} is a core skill on this lamp: it comes with the lamp, \
                 and an installable skill can't take its place"
            ));
        }
        Ok(_) => Some(version_of(&target)),
    };
    // A function is one skill's: one that takes another's name would take
    // it from the model, a core skill's first ([`load_each`]).
    if let Some(tool) = &skill.tool
        && let Some((holder, kind)) = tool_holder(skills, name, tool)
    {
        return Err(format!(
            "not installed: {name}'s tool {tool:?} is the {} skill {holder}'s on this lamp: \
             a function the model calls is one skill's",
            kind.name()
        ));
    }
    let failed = |error: io::Error| format!("not installed: {}: {error}", target.display());
    match &replaced {
        None => fs::rename(folder, &target).map_err(failed)?,
        Some(_) => {
            // The old out of the way, the new in its place, at once: the
            // program looks every 2 s, and between the two it may see none.
            let old = hidden_place(skills, "removed", name);
            if old.exists() {
                fs::remove_dir_all(&old).map_err(failed)?;
            }
            fs::rename(&target, &old).map_err(failed)?;
            if let Err(error) = fs::rename(folder, &target) {
                let _ = fs::rename(&old, &target);
                return Err(failed(error));
            }
            let _ = fs::remove_dir_all(&old);
        }
    }
    let version = skill.version.as_deref().unwrap_or("with no version");
    let what = match replaced {
        None => format!("installed {name} {version} (installable)"),
        Some(before) => format!(
            "replaced {name} {} with {version} (installable)",
            before.as_deref().unwrap_or("with no version")
        ),
    };
    let from = match package {
        None => format!("from its folder, {count} files"),
        Some(package) => format!(
            "from its package, {count} files, each one's size and sha256 as {MANIFEST} says; {}",
            if package.signature.is_some() {
                "its signature not checked: signing is not built yet"
            } else {
                "it carries no signature, and none is checked yet: signing is not built"
            }
        ),
    };
    let (loaded, _) = super::load_all(skills);
    Ok(format!(
        "{what} {from}. It may use: {}. A running lamp-skills loads it within 2 s. {}",
        skill.permissions.said(),
        functions_said(&loaded)
    ))
}

/// Removes the installable skill `name` from `skills` and says so. A core
/// skill is refused: it comes with the lamp.
pub fn remove(skills: &Path, name: &str) -> Result<String, String> {
    match taken_off(skills, name, None)? {
        None => Ok(removed_said(name)),
        Some(left) => Err(format!("removed {name}, but {left}")),
    }
}

/// What a remove says once the skill is gone for the running program.
fn removed_said(name: &str) -> String {
    format!("removed {name}: a running lamp-skills lets it go within 2 s")
}

/// [`remove`]'s steps, the folder's lock waited for at most `wait` (always,
/// when `None`). Once the skill is renamed aside it is gone for the running
/// program: `Some` says where it was set aside and why that was not then
/// deleted (the next install deletes it, [`left_by_a_run`]).
fn taken_off(skills: &Path, name: &str, wait: Option<Duration>) -> Result<Option<String>, String> {
    if !skill_name_ok(name) {
        return Err(format!(
            "{name:?} is not a skill's name: 1 to 64 lowercase letters, digits and hyphens"
        ));
    }
    let target = skills.join(name);
    match fs::symlink_metadata(&target) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => {
            return Err(format!(
                "{} is not a skill's folder: left as it is",
                target.display()
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(format!("no skill {name} on this lamp"));
        }
        Err(error) => return Err(format!("{}: {error}", target.display())),
    }
    let _held = match wait {
        None => locked(skills)?,
        Some(wait) => locked_within(skills, wait)?,
    };
    if kind_of(&target) == SkillKind::Core {
        return Err(format!(
            "{name} is a core skill: it comes with the lamp and can't be removed"
        ));
    }
    // Gone at once for the running program, then deleted.
    let old = hidden_place(skills, "removed", name);
    let failed = |error: io::Error| format!("not removed: {}: {error}", target.display());
    if old.exists() {
        fs::remove_dir_all(&old).map_err(failed)?;
    }
    fs::rename(&target, &old).map_err(failed)?;
    Ok(fs::remove_dir_all(&old)
        .err()
        .map(|error| format!("{} was not deleted: {error}", old.display())))
}

/// Where the manage helper listens on a product lamp
/// (`deploy/product/lamp-skills-manage.socket`): root's socket, which only
/// the group `lampos` may connect to. systemd holds it and starts
/// `lamp-skills asked` as root for each connection, with the connection as
/// its standard input and output ([`asked`]).
pub const MANAGE_SOCKET: &str = "/run/lampos-skills-manage/manage.sock";

/// The most bytes of a request to [`asked`].
pub const ASKED_MOST: u64 = 4_096;

/// One request to the manage helper, read from `input`, done in `skills`,
/// and answered on `output` in one line: `{"done": WHAT}` or
/// `{"refused": WHY}`; true when it was done. The one request taken is
/// `{"remove": NAME}`, a skill taken off by [`remove`], which checks it
/// itself, whoever asked: a name that is not a skill's and a core skill are
/// refused here, not only by the asker (`lamp-cloud`'s `skills.remove`,
/// #1722). It waits at most `lock_wait` ([`ASKED_LOCK_WAIT`] on the lamp,
/// shorter only in a test) for the folder, then refuses
/// with [`BUSY`] having done nothing; a skill renamed aside whose old folder
/// was not deleted is answered done, that folder's path in the journal only. `install` is not taken: it will be, for a package from the update
/// channel under its key's signature alone, once the channel carries skills.
pub fn asked(
    skills: &Path,
    lock_wait: Duration,
    input: impl Read,
    mut output: impl io::Write,
) -> io::Result<bool> {
    let mut line = String::new();
    // A request past its bound, or not UTF-8, is no request.
    let _ = io::BufReader::new(input.take(ASKED_MOST)).read_line(&mut line);
    let request: Option<serde_json::Value> = serde_json::from_str(line.trim_end()).ok();
    let only = |key: &str| {
        request
            .as_ref()
            .and_then(serde_json::Value::as_object)
            .filter(|asked| asked.len() == 1)
            .and_then(|asked| asked.get(key))
    };
    let done = match (only("remove"), only("install")) {
        // Renamed aside is removed: where it was set aside, and why it
        // was not then deleted, go to the journal alone.
        (Some(serde_json::Value::String(name)), _) => {
            taken_off(skills, name, Some(lock_wait)).map(|left| {
                if let Some(left) = left {
                    eprintln!("lamp-skills: asked: removed {name}, but {left}");
                }
                removed_said(name)
            })
        }
        (_, Some(_)) => Err(
            "install is not taken here yet: only a package from the update channel, under its key's \
             signature, will be"
                .to_owned(),
        ),
        _ => Err("a request is one line, {\"remove\": NAME}".to_owned()),
    };
    let answer = match &done {
        Ok(said) => serde_json::json!({ "done": said }),
        Err(why) => serde_json::json!({ "refused": why }),
    };
    eprintln!("lamp-skills: asked: {answer}");
    writeln!(output, "{answer}")?;
    output.flush()?;
    Ok(done.is_ok())
}

/// The skills in `skills`, a line each: name, kind, version and what it
/// may use, or why it does not load, as the running program loads them
/// ([`load_each`]); then the functions they give the model.
pub fn list(skills: &Path) -> Result<String, String> {
    fs::read_dir(skills).map_err(|error| format!("{}: {error}", skills.display()))?;
    let mut loaded = Vec::new();
    let mut rows = vec![[
        "NAME".to_owned(),
        "KIND".to_owned(),
        "VERSION".to_owned(),
        "MAY USE".to_owned(),
    ]];
    for (folder, skill) in load_each(skills) {
        let name = folder
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        rows.push(match skill {
            Ok(skill) => {
                let row = [
                    skill.name.clone(),
                    skill.kind.name().to_owned(),
                    skill.version.clone().unwrap_or_else(|| "-".into()),
                    if skill.enabled {
                        skill.permissions.said()
                    } else {
                        format!("disabled; {}", skill.permissions.said())
                    },
                ];
                loaded.push(skill);
                row
            }
            Err(why) => [
                name,
                kind_of(&folder).name().to_owned(),
                version_of(&folder).unwrap_or_else(|| "-".into()),
                format!("not loaded: {why}"),
            ],
        });
    }
    let widths: Vec<usize> = (0..3)
        .map(|column| rows.iter().map(|row| row[column].len()).max().unwrap_or(0))
        .collect();
    let mut listed: String = rows
        .iter()
        .map(|row| {
            format!(
                "{:<w0$}  {:<w1$}  {:<w2$}  {}\n",
                row[0],
                row[1],
                row[2],
                row[3],
                w0 = widths[0],
                w1 = widths[1],
                w2 = widths[2]
            )
        })
        .collect();
    listed.push_str(&functions_said(&loaded));
    listed.push('\n');
    Ok(listed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file swapped after its folder was listed, for a link (to a file
    /// only root may read, run as root), another file or a fifo, is refused
    /// as it is opened and never read; the fifo at once, not waited on
    /// (the reviewer on #1698, 08:39).
    #[test]
    fn a_file_swapped_after_its_folder_was_listed_is_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("SKILL.md");
        let secret = dir.path().join("secret");
        fs::write(&secret, b"root's alone").unwrap();
        fs::write(&path, b"a skill").unwrap();
        let listed = fs::symlink_metadata(&path).unwrap();
        let mut read = String::new();
        open_plain(&path, &listed)
            .unwrap()
            .read_to_string(&mut read)
            .unwrap();
        assert_eq!(read, "a skill");
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&secret, &path).unwrap();
        let why = open_plain(&path, &listed).unwrap_err();
        assert_eq!(why, "a link: a skill is plain files and folders");
        // Another plain file in its place: the same name, not the same file.
        fs::remove_file(&path).unwrap();
        fs::hard_link(&secret, &path).unwrap();
        let why = open_plain(&path, &listed).unwrap_err();
        assert!(why.starts_with("changed while it was read"), "{why}");
        fs::remove_file(&path).unwrap();
        let made = std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap();
        assert!(made.success());
        let why = open_plain(&path, &listed).unwrap_err();
        assert_eq!(why, "not a plain file");
    }

    /// Only what an install or a remove names its own folders is taken for
    /// one a killed run left.
    #[test]
    fn only_a_runs_own_folder_names_are_taken_for_what_it_left() {
        for name in [
            ".install-desk-tutor-4242",
            ".removed-desk-tutor-1",
            ".install-a-0",
        ] {
            assert!(left_by_a_run(name), "{name}");
        }
        for name in [
            "desk-tutor",
            "install-desk-tutor-4242",
            ".install-desk-tutor",
            ".install-desk-tutor-",
            ".install--4242",
            ".install-Desk-4242",
            ".install-desk_tutor-4242",
            ".install-desk-tutor-42a",
            ".install-desk-tutor-12345678901",
            ".installer-desk-tutor-4242",
            ".removed-../x-1",
            ".lamp-skills.lock",
        ] {
            assert!(!left_by_a_run(name), "{name}");
        }
    }
}
