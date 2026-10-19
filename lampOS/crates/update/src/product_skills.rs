//! The product's skill folders, as a build carries them (docs/updates.md,
//! "The product's skills"). A round places them, so a lamp gets each
//! build's own and not only those of the updater install left (#2014's P2).
//!
//! `lamp-update manifest` writes every file of each
//! [`PRODUCT_SKILLS`](crate::install::PRODUCT_SKILLS) folder of
//! `lampOS/skills/` but its `tests/` into one file, [`BUNDLE`], listed in
//! the manifest's `files` with its SHA-256 and size as a binary is, so it
//! is signed with the manifest. An updater from before this file fetches
//! only its parts' binaries and passes it over: no field is added to the
//! manifest, which an older updater would refuse whole.
//!
//! Each file is one of three, as #1913 decides: the build's before (its
//! SHA-256 in the state's `settings`, as install or the last round wrote
//! it), the build's now (the bundle), and the lamp's now (the file). Only
//! one still the build's before, or one never there and never recorded, is
//! written; one the lamp changed or removed is kept, and said. A folder the
//! bundle does not name (a skill the person installed) is never read or
//! written.
use crate::{
    manifest,
    records::{SkillBack, write_whole},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
};

/// The bundle's name among a build's files.
pub const BUNDLE: &str = "lamp-product-skills";
/// Its format: what this program reads.
pub const FORMAT: &str = "lampos-skills/1";
/// The folder install writes the product skills into, and the only one a
/// round places them in: a lamp-skills unit with another `--dir` (a lamp's
/// own, as lamp-4ace's) is not install's, and is left alone.
pub const FOLDER: &str = "/var/lib/lampOS/skills";
/// The largest bundle read: today's six skills are about 30 KB.
pub const MOST_BYTES: u64 = 1024 * 1024;
/// At most this many files in one bundle.
pub const MOST_FILES: usize = 256;

/// One file of a product skill: its path under [`FOLDER`]
/// (`welcome-back/lights/warm_glow.toml`), and its text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillFile {
    pub path: String,
    pub text: String,
}

/// The product skills of a build.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bundle {
    pub format: String,
    /// The skills it carries, by name: a folder of each.
    pub skills: Vec<String>,
    pub files: Vec<SkillFile>,
}

fn invalid(why: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, why)
}

/// A skill's name as a folder: lowercase letters, digits and `-`.
fn is_skill_name(name: &str) -> bool {
    manifest::is_token(name)
}

/// A file's path in a skill: plain parts, none hidden, none a test's.
fn is_skill_path(path: &str) -> bool {
    let mut parts = path.split('/');
    let Some(skill) = parts.next() else {
        return false;
    };
    let rest: Vec<&str> = parts.collect();
    path.len() < 256
        && is_skill_name(skill)
        && !rest.is_empty()
        && rest.first() != Some(&"tests")
        && rest.iter().all(|part| {
            !part.is_empty()
                && !part.starts_with('.')
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        })
}

/// The bundle of these skills of `skills` (`lampOS/skills`): every file of
/// each folder but its `tests/` and hidden files, sorted. Each must be text.
pub fn make(skills: &Path, names: &[&str]) -> io::Result<Bundle> {
    fn walk(folder: &Path, under: &str, files: &mut Vec<SkillFile>) -> io::Result<()> {
        for entry in fs::read_dir(folder)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = format!("{under}/{name}");
            if name.starts_with('.') {
                continue;
            }
            let kind = entry.file_type()?;
            if kind.is_dir() {
                if !under.contains('/') && name == "tests" {
                    continue;
                }
                walk(&entry.path(), &path, files)?;
            } else if kind.is_file() {
                if !is_skill_path(&path) {
                    return Err(invalid(format!("{path} is not a plain path in a skill")));
                }
                let text = String::from_utf8(fs::read(entry.path())?)
                    .map_err(|_| invalid(format!("{path} is not text")))?;
                files.push(SkillFile { path, text });
            } else {
                return Err(invalid(format!("{path} is neither a file nor a folder")));
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    for name in names {
        if !is_skill_name(name) {
            return Err(invalid(format!("{name:?} is not a skill's name")));
        }
        let folder = skills.join(name);
        if !folder.join("skill.toml").is_file() {
            return Err(invalid(format!("{} has no skill.toml", folder.display())));
        }
        walk(&folder, name, &mut files)?;
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let mut skills: Vec<String> = names.iter().map(|name| (*name).to_owned()).collect();
    skills.sort();
    let bundle = Bundle {
        format: FORMAT.to_owned(),
        skills,
        files,
    };
    check(&bundle).map_err(invalid)?;
    Ok(bundle)
}

/// The bundle's bytes, as published.
pub fn bytes(bundle: &Bundle) -> io::Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(bundle).map_err(io::Error::other)?;
    bytes.push(b'\n');
    if bytes.len() as u64 > MOST_BYTES {
        return Err(invalid(format!(
            "the product skills are {} bytes, and at most {MOST_BYTES}",
            bytes.len()
        )));
    }
    Ok(bytes)
}

/// A bundle from its bytes, checked: its format, every path plain and in a
/// skill it names, nothing twice, every skill with its skill.toml.
pub fn parse(bytes: &[u8]) -> Result<Bundle, String> {
    if bytes.len() as u64 > MOST_BYTES {
        return Err(format!("{BUNDLE} is too large"));
    }
    let bundle: Bundle = serde_json::from_slice(bytes)
        .map_err(|error| format!("{BUNDLE} cannot be read: {error}"))?;
    check(&bundle)?;
    Ok(bundle)
}

fn check(bundle: &Bundle) -> Result<(), String> {
    if bundle.format != FORMAT {
        return Err(format!("{BUNDLE} is {:?}, not {FORMAT}", bundle.format));
    }
    if bundle.files.len() > MOST_FILES {
        return Err(format!("{BUNDLE} has more than {MOST_FILES} files"));
    }
    let skills: BTreeSet<&str> = bundle.skills.iter().map(String::as_str).collect();
    if skills.len() != bundle.skills.len() || !skills.iter().all(|name| is_skill_name(name)) {
        return Err(format!("{BUNDLE}'s skills are not plain names, once each"));
    }
    let mut paths = BTreeSet::new();
    for file in &bundle.files {
        let skill = file.path.split('/').next().unwrap_or_default();
        if !is_skill_path(&file.path) || !skills.contains(skill) {
            return Err(format!(
                "{:?} is not a path in a skill {BUNDLE} carries",
                file.path
            ));
        }
        if !paths.insert(file.path.as_str()) {
            return Err(format!("{} is in {BUNDLE} twice", file.path));
        }
    }
    for skill in &skills {
        if !paths.contains(format!("{skill}/skill.toml").as_str()) {
            return Err(format!("{BUNDLE} carries {skill} without its skill.toml"));
        }
    }
    Ok(())
}

/// What a file of the bundle needs on this lamp.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    /// Written: the lamp has the build's before, or never had it.
    Write,
    /// Already the build's now: only recorded.
    Record,
    /// The lamp's own: kept, with why.
    Keep(&'static str),
}

/// One file, where it goes on the lamp (`/var/lib/lampOS/skills/…`), and
/// what it needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub path: String,
    pub text: String,
    pub sha256: String,
    pub act: Act,
}

fn on_root(root: &Path, path: &str) -> PathBuf {
    root.join(path.trim_start_matches('/'))
}

fn now_sha(root: &Path, path: &str) -> io::Result<Option<String>> {
    match fs::read(on_root(root, path)) {
        Ok(bytes) => Ok(Some(manifest::sha256_hex(&bytes))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io::Error::new(error.kind(), format!("{path}: {error}"))),
    }
}

/// What each file of the bundle needs, from the three: the build's before
/// (`recorded`, or `writing` for one install was about to write), the
/// build's now, and the lamp's now. Nothing to do is left out.
pub fn plan(
    root: &Path,
    bundle: &Bundle,
    recorded: &BTreeMap<String, String>,
    writing: &BTreeMap<String, String>,
) -> io::Result<Vec<Step>> {
    let mut steps = Vec::new();
    for file in &bundle.files {
        let path = format!("{FOLDER}/{}", file.path);
        let new = manifest::sha256_hex(file.text.as_bytes());
        let builds =
            |sha: &String| recorded.get(&path) == Some(sha) || writing.get(&path) == Some(sha);
        let act = match now_sha(root, &path)? {
            Some(now) if now == new => (recorded.get(&path) != Some(&new)).then_some(Act::Record),
            Some(now) if builds(&now) => Some(Act::Write),
            Some(_) => Some(Act::Keep("this lamp's own")),
            None if recorded.contains_key(&path) => Some(Act::Keep("removed on this lamp")),
            None => Some(Act::Write),
        };
        if let Some(act) = act {
            steps.push(Step {
                path,
                text: file.text.clone(),
                sha256: new,
                act,
            });
        }
    }
    Ok(steps)
}

/// Whether a plan writes anything.
pub fn writes(steps: &[Step]) -> bool {
    steps.iter().any(|step| step.act == Act::Write)
}

/// What each file to be written is before it is: its text (or none, not
/// there), what is to be written, and the folders to be made for it, the
/// deepest first. Read just before the write, and kept in the journal
/// first, so a put-back restores it.
pub fn backs(root: &Path, steps: &[Step]) -> io::Result<Vec<SkillBack>> {
    let mut made_already = BTreeSet::new();
    let mut backs = Vec::new();
    for step in steps.iter().filter(|step| step.act == Act::Write) {
        let before = match fs::read(on_root(root, &step.path)) {
            Ok(bytes) => Some(String::from_utf8(bytes).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{} is not text", step.path),
                )
            })?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(io::Error::new(
                    error.kind(),
                    format!("{}: {error}", step.path),
                ));
            }
        };
        let mut made = Vec::new();
        let mut folder = Path::new(&step.path).parent();
        while let Some(at) = folder {
            let text = at.display().to_string();
            if text.len() <= FOLDER.len() || on_root(root, &text).exists() {
                break;
            }
            if made_already.insert(text.clone()) {
                made.push(text);
            }
            folder = at.parent();
        }
        backs.push(SkillBack {
            path: step.path.clone(),
            before,
            wrote: step.sha256.clone(),
            made,
        });
    }
    Ok(backs)
}

/// Each file written whole, 0644, its folders made 0755, as install writes
/// them. A file that cannot be written stops the rest.
pub fn write(root: &Path, steps: &[Step]) -> Result<Vec<String>, String> {
    let mut wrote = Vec::new();
    for step in steps.iter().filter(|step| step.act == Act::Write) {
        let at = on_root(root, &step.path);
        let failed = |error: io::Error| format!("{}: {error}", step.path);
        if let Some(parent) = at.parent() {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o755)
                .create(parent)
                .map_err(failed)?;
        }
        write_whole(&at, step.text.as_bytes(), 0o644).map_err(failed)?;
        wrote.push(step.path.clone());
    }
    Ok(wrote)
}

/// Each file a round wrote put back as it was, the last first: its text,
/// or gone, and the folders made for it removed if still empty. One the
/// lamp changed again since (no longer what was written) is left, and
/// said. Every file is tried, whatever happens to one.
pub fn restore(root: &Path, backs: &[SkillBack], say: &mut dyn FnMut(&str)) -> Result<(), String> {
    let mut troubles = Vec::new();
    for back in backs.iter().rev() {
        let at = on_root(root, &back.path);
        match now_sha(root, &back.path) {
            Ok(Some(now)) if now == back.wrote => {}
            // Never written (the switch stopped before it), or already
            // put back.
            Ok(None) if back.before.is_none() => {}
            Ok(Some(now))
                if back
                    .before
                    .as_ref()
                    .is_some_and(|text| manifest::sha256_hex(text.as_bytes()) == now) =>
            {
                continue;
            }
            Ok(_) => {
                say(&format!(
                    "kept {}: changed on this lamp since the round wrote it",
                    back.path
                ));
                continue;
            }
            Err(error) => {
                troubles.push(error.to_string());
                continue;
            }
        }
        let result = match &back.before {
            Some(text) => write_whole(&at, text.as_bytes(), 0o644),
            None => match fs::remove_file(&at) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
                _ => Ok(()),
            },
        };
        if let Err(error) = result {
            troubles.push(format!("{} cannot be put back: {error}", back.path));
            continue;
        }
        for folder in &back.made {
            // Only if empty: a folder something else has put a file in
            // since is left.
            let _ = fs::remove_dir(on_root(root, folder));
        }
    }
    if troubles.is_empty() {
        Ok(())
    } else {
        Err(troubles.join("; "))
    }
}

/// `folder` copied into `into` (made afresh), with the plan's writes: the
/// folder as the round would leave it, for the guard to judge before
/// anything of the lamp's is written. Links are copied as links.
pub fn stage(root: &Path, folder: &str, steps: &[Step], into: &Path) -> io::Result<()> {
    fn copy(from: &Path, to: &Path) -> io::Result<()> {
        fs::DirBuilder::new().mode(0o755).create(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            let target = to.join(entry.file_name());
            if kind.is_dir() {
                copy(&entry.path(), &target)?;
            } else if kind.is_symlink() {
                std::os::unix::fs::symlink(fs::read_link(entry.path())?, &target)?;
            } else if kind.is_file() {
                fs::copy(entry.path(), &target)?;
            }
        }
        Ok(())
    }
    if fs::symlink_metadata(into).is_ok() {
        fs::remove_dir_all(into)?;
    }
    let from = on_root(root, folder);
    if from.is_dir() {
        copy(&from, into)?;
    } else {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o755)
            .create(into)?;
    }
    for step in steps.iter().filter(|step| step.act == Act::Write) {
        let rest = step
            .path
            .strip_prefix(folder)
            .unwrap_or(&step.path)
            .trim_start_matches('/');
        let at = into.join(rest);
        if let Some(parent) = at.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(at, &step.text)?;
    }
    Ok(())
}

/// Whether `folder` can be written by this program: a file made and removed
/// in it. A round run under a unit from before #2014's P2 cannot
/// (`ProtectSystem=strict` without the skills folder in `ReadWritePaths`).
pub fn writable(root: &Path, folder: &str) -> io::Result<()> {
    let at = on_root(root, folder);
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o755)
        .create(&at)?;
    let probe = at.join(".lamp-update-probe");
    fs::write(&probe, b"")?;
    fs::remove_file(&probe)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle(files: &[(&str, &str)]) -> Bundle {
        let mut skills: Vec<String> = files
            .iter()
            .map(|(path, _)| path.split('/').next().unwrap().to_owned())
            .collect();
        skills.dedup();
        Bundle {
            format: FORMAT.into(),
            skills,
            files: files
                .iter()
                .map(|(path, text)| SkillFile {
                    path: (*path).into(),
                    text: (*text).into(),
                })
                .collect(),
        }
    }

    #[test]
    fn only_plain_paths_in_the_skills_it_names_are_taken() {
        let good = bundle(&[("dance/skill.toml", "a"), ("dance/lights/x.toml", "b")]);
        assert!(parse(&bytes(&good).unwrap()).is_ok());
        for wrong in [
            bundle(&[("dance/skill.toml", "a"), ("dance/../x", "b")]),
            bundle(&[("dance/skill.toml", "a"), ("dance/.hidden", "b")]),
            bundle(&[("dance/skill.toml", "a"), ("dance/tests/t.toml", "b")]),
            bundle(&[("dance/skill.toml", "a"), ("dance/skill.toml", "b")]),
            bundle(&[("Dance/skill.toml", "a")]),
            bundle(&[("dance/SKILL.md", "a")]),
        ] {
            assert!(parse(&bytes(&wrong).unwrap()).is_err(), "{wrong:?}");
        }
        let mut other = good.clone();
        other.files[0].path = "reminders/skill.toml".into();
        assert!(parse(&bytes(&other).unwrap()).is_err());
        other = good;
        other.format = "lampos-skills/2".into();
        assert!(parse(&bytes(&other).unwrap()).is_err());
    }

    #[test]
    fn each_file_is_one_of_three() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path();
        let put = |path: &str, text: &str| {
            let at = on_root(root, path);
            fs::create_dir_all(at.parent().unwrap()).unwrap();
            fs::write(at, text).unwrap();
        };
        let sha = |text: &str| manifest::sha256_hex(text.as_bytes());
        let p = |rest: &str| format!("{FOLDER}/{rest}");
        put(&p("a/skill.toml"), "old");
        put(&p("b/skill.toml"), "mine");
        put(&p("c/skill.toml"), "new");
        put(&p("e/skill.toml"), "old");
        let recorded = BTreeMap::from([
            (p("a/skill.toml"), sha("old")),
            (p("b/skill.toml"), sha("old")),
            (p("d/skill.toml"), sha("old")),
        ]);
        let writing = BTreeMap::from([(p("e/skill.toml"), sha("old"))]);
        let new = bundle(&[
            ("a/skill.toml", "new"),
            ("b/skill.toml", "new"),
            ("c/skill.toml", "new"),
            ("d/skill.toml", "new"),
            ("e/skill.toml", "new"),
            ("f/skill.toml", "new"),
        ]);
        let acts: Vec<(String, Act)> = plan(root, &new, &recorded, &writing)
            .unwrap()
            .into_iter()
            .map(|step| (step.path, step.act))
            .collect();
        assert_eq!(
            acts,
            [
                (p("a/skill.toml"), Act::Write),
                (p("b/skill.toml"), Act::Keep("this lamp's own")),
                (p("c/skill.toml"), Act::Record),
                (p("d/skill.toml"), Act::Keep("removed on this lamp")),
                (p("e/skill.toml"), Act::Write),
                (p("f/skill.toml"), Act::Write),
            ]
        );
    }

    #[test]
    fn what_is_written_is_put_back_and_a_file_changed_since_is_left() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path();
        let at = |rest: &str| on_root(root, &format!("{FOLDER}/{rest}"));
        fs::create_dir_all(at("a")).unwrap();
        fs::write(at("a/skill.toml"), "old").unwrap();
        fs::write(at("a/SKILL.md"), "old").unwrap();
        let new = bundle(&[
            ("a/skill.toml", "new"),
            ("a/SKILL.md", "new"),
            ("f/lights/x.toml", "new"),
            ("f/skill.toml", "new"),
        ]);
        let recorded = BTreeMap::from([
            (
                format!("{FOLDER}/a/skill.toml"),
                manifest::sha256_hex(b"old"),
            ),
            (format!("{FOLDER}/a/SKILL.md"), manifest::sha256_hex(b"old")),
        ]);
        let steps = plan(root, &new, &recorded, &BTreeMap::new()).unwrap();
        let backs = backs(root, &steps).unwrap();
        write(root, &steps).unwrap();
        assert_eq!(fs::read_to_string(at("f/lights/x.toml")).unwrap(), "new");
        // The lamp changes one file after the round wrote it.
        fs::write(at("a/SKILL.md"), "the lamp's").unwrap();
        let mut said = Vec::new();
        restore(root, &backs, &mut |line| said.push(line.to_owned())).unwrap();
        assert_eq!(fs::read_to_string(at("a/skill.toml")).unwrap(), "old");
        assert_eq!(fs::read_to_string(at("a/SKILL.md")).unwrap(), "the lamp's");
        assert!(!at("f").exists(), "the folder made for f is left");
        assert!(
            said.iter().any(|line| line.contains("a/SKILL.md")),
            "{said:?}"
        );
        // Twice is the same as once.
        restore(root, &backs, &mut |_| {}).unwrap();
        assert_eq!(fs::read_to_string(at("a/skill.toml")).unwrap(), "old");
    }

    /// The bundle CI makes from `lampOS/skills` is, file for file, what
    /// install writes from `install::SETTINGS`: a lamp gets the same
    /// product skills from a round as from an install of that build.
    #[test]
    fn the_bundle_of_the_repository_is_what_install_writes() {
        let skills = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills");
        let made = make(&skills, crate::install::PRODUCT_SKILLS).unwrap();
        let from_bundle: BTreeMap<String, String> = made
            .files
            .iter()
            .map(|file| (format!("{FOLDER}/{}", file.path), file.text.clone()))
            .collect();
        let from_install: BTreeMap<String, String> = crate::install::SETTINGS
            .iter()
            .filter(|(path, _)| path.starts_with(&format!("{FOLDER}/")))
            .map(|(path, text)| ((*path).to_owned(), (*text).to_owned()))
            .collect();
        assert_eq!(from_bundle, from_install);
        assert!(bytes(&made).unwrap().len() < MOST_BYTES as usize);
    }
}
