//! A skill's files, read from its page (#8629; the owner: every skill's
//! files readable from its page, built-ins included; the first builder's
//! three conditions, 19:23 on 25 Nov): `skills.files {"name"}`, the plain
//! files under a skill's folder whatever its group, each with its size,
//! and `skills.files {"name", "path"}`, one UTF-8 text file. Read only,
//! with the skills page (`LAMP_SKILLS_PAGE=on`) and no switch of its own:
//! it reads what the page already shows for a made skill through
//! `skills.share`'s preview. Nothing is sent anywhere and nothing is
//! written.
//!
//! Never listed or read: a dot file (`.package.json`, `.pushed.json`, the
//! owner's `.yes.json`), [`SIGNATURE`], and anything outside the skill's
//! folder; the skills state folder (yeses, counts, secrets) is beside the
//! skills folder, never under a skill. A link is shown as one and never
//! followed, listing or reading: a path under one is refused as `..` is
//! (`docs/cloud.md`, "A skill's files").

use serde_json::{Value, json};
use std::collections::VecDeque;
use std::fs;
use std::io::Read;
use std::path::{Component, Path};

use crate::skills::{Group, Places};

/// The kind, answered with the page's.
pub const KIND: &str = "skills.files";

/// The most entries one list holds (the first builder's second condition):
/// past it the list stops and says `"cut": true`, so one odd folder can't
/// make a reply the app chokes on.
pub const MOST_ENTRIES: usize = 500;

/// The most characters of one file's text, the preview's cap: over it the
/// text comes back cut at the cap with `"cut": true`, never refused, so a
/// long test recording still opens (the first builder's third condition).
pub const TEXT_MOST: usize = crate::share::PREVIEW_MOST;

/// The most bytes read of a file: four a character at the cap, so a text
/// cut at [`READ_MOST`] still holds [`TEXT_MOST`] characters.
const READ_MOST: usize = TEXT_MOST * 4;

/// The Store's signature over a package, never shown.
pub const SIGNATURE: &str = "lamp-skill.sig";

/// What a path that is not a plain path inside the skill is told: `..`, a
/// dot part, an absolute path, an empty one, or a link anywhere along it.
pub const NOT_IN_SKILL: &str = "not a path in the skill";

/// The longest `path` taken, in bytes.
const PATH_MOST: usize = 1_024;

/// Where a skill came from, as the page says it: `built-in` (a core skill,
/// from the build), `store` (an installed one), `made` (made on this lamp,
/// `made: here`) or `pushed` (a paired computer's, in the pushed folder).
pub fn source(group: Group) -> &'static str {
    match group {
        Group::Core => "built-in",
        Group::Installed => "store",
        Group::MadeHere => "made",
        Group::Pushed => "pushed",
    }
}

/// `skills.files {"name"}` → `{"files": {"name", "source", "files":
/// [{"path", "size"} | {"path", "link": true}], "cut"?: true}}`, sorted by
/// path; `skills.files {"name", "path"}` → `{"file": {"path", "text",
/// "cut"?: true}}`. The name becomes a folder as the page's rows do
/// ([`crate::skills::located`]): a name that is not a skill's is refused
/// without being said back, and one no skill has is `no skill named NAME
/// on this lamp`.
pub fn files(places: &Places, data: &Value) -> Result<Value, String> {
    let name = data
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| lamp_skills::package::skill_name_ok(name))
        .ok_or_else(|| crate::skills::NOT_A_NAME.to_owned())?;
    fs::read_dir(&places.folder).map_err(|_| "the lamp's skills can't be read now".to_owned())?;
    let (folder, group) = crate::skills::located(places, name)
        .ok_or_else(|| format!("no skill named {name} on this lamp"))?;
    match data.get("path") {
        None | Some(Value::Null) => {
            listed(&folder, name, source(group)).map(|page| json!({ "files": page }))
        }
        Some(path) => {
            let path = path.as_str().ok_or_else(|| NOT_IN_SKILL.to_owned())?;
            read(&folder, name, path).map(|file| json!({ "file": file }))
        }
    }
}

/// What a read that fails says, never the lamp's path.
fn unread(error: std::io::Error) -> String {
    format!("the skill could not be read: {error}")
}

/// The plain files under `folder`, walked by folder and name, each as
/// `{"path", "size"}`; a link as `{"path", "link": true}`, never followed;
/// a dot file, [`SIGNATURE`] and anything that is neither a file, a folder
/// nor a link left out. At most [`MOST_ENTRIES`], then `cut`.
fn listed(folder: &Path, name: &str, source: &str) -> Result<Value, String> {
    let mut files = Vec::new();
    let mut cut = false;
    let mut folders = VecDeque::from([folder.to_path_buf()]);
    'walk: while let Some(dir) = folders.pop_front() {
        let mut entries: Vec<_> = fs::read_dir(&dir)
            .map_err(unread)?
            .filter_map(Result::ok)
            .collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            if file_name.starts_with('.') || file_name == SIGNATURE {
                continue;
            }
            let path = entry.path();
            // The entry itself, never what a link points at.
            let Ok(meta) = fs::symlink_metadata(&path) else {
                continue;
            };
            let shown = path
                .strip_prefix(folder)
                .map(|rest| rest.to_string_lossy().into_owned())
                .unwrap_or_default();
            let row = if meta.file_type().is_symlink() {
                json!({"path": shown, "link": true})
            } else if meta.is_dir() {
                folders.push_back(path);
                continue;
            } else if meta.is_file() {
                json!({"path": shown, "size": meta.len()})
            } else {
                continue;
            };
            if files.len() >= MOST_ENTRIES {
                cut = true;
                break 'walk;
            }
            files.push(row);
        }
    }
    files.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    let mut page = json!({"name": name, "source": source, "files": files});
    if cut {
        page["cut"] = json!(true);
    }
    Ok(page)
}

/// `path`'s parts when it is a plain path inside the skill: not empty, not
/// absolute, no `.` or `..`, no part beginning with a dot, no control
/// character, at most [`PATH_MOST`] bytes.
fn parts_of(path: &str) -> Option<Vec<&str>> {
    if path.is_empty() || path.len() > PATH_MOST || path.chars().any(char::is_control) {
        return None;
    }
    let mut parts = Vec::new();
    for component in Path::new(path).components() {
        let Component::Normal(part) = component else {
            return None;
        };
        let part = part.to_str()?;
        if part.starts_with('.') {
            return None;
        }
        parts.push(part);
    }
    (!parts.is_empty()).then_some(parts)
}

/// One file of the skill as `{"path", "text", "cut"?: true}`: each part of
/// `path` looked at as it is (`lstat`), a link anywhere along it refused as
/// [`NOT_IN_SKILL`]; a part that isn't there, a folder or [`SIGNATURE`] is
/// `no file PATH in NAME`; bytes that are not UTF-8 are `not a text file:
/// PATH`. The text is cut at [`TEXT_MOST`] characters, said so.
fn read(folder: &Path, name: &str, path: &str) -> Result<Value, String> {
    let parts = parts_of(path).ok_or_else(|| NOT_IN_SKILL.to_owned())?;
    let path = parts.join("/");
    let no_file = || format!("no file {path} in {name}");
    let mut at = folder.to_path_buf();
    let mut meta = None;
    for part in &parts {
        at.push(part);
        let found = fs::symlink_metadata(&at).map_err(|_| no_file())?;
        if found.file_type().is_symlink() {
            return Err(NOT_IN_SKILL.to_owned());
        }
        meta = Some(found);
    }
    if !meta.is_some_and(|meta| meta.is_file()) || parts.last() == Some(&SIGNATURE) {
        return Err(no_file());
    }
    let mut bytes = Vec::new();
    fs::File::open(&at)
        .map_err(unread)?
        .take(READ_MOST as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(unread)?;
    let more = bytes.len() > READ_MOST;
    bytes.truncate(READ_MOST);
    let text = match std::str::from_utf8(&bytes) {
        Ok(text) => text,
        // Only the read's own end cut a character: text, read to there.
        Err(error) if more && error.error_len().is_none() => {
            std::str::from_utf8(&bytes[..error.valid_up_to()]).unwrap_or_default()
        }
        Err(_) => return Err(format!("not a text file: {path}")),
    };
    let kept: String = text.chars().take(TEXT_MOST).collect();
    let cut = more || kept.len() < text.len();
    let mut file = json!({"path": path, "text": kept});
    if cut {
        file["cut"] = json!(true);
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plain path has normal parts only: no `.`, `..`, root, dot part,
    /// control character or empty path; `a//b` and `a/` read as their
    /// parts.
    #[test]
    fn a_plain_path_inside_the_skill_has_normal_parts_only() {
        assert_eq!(parts_of("SKILL.md"), Some(vec!["SKILL.md"]));
        assert_eq!(
            parts_of("moves//wave.json/"),
            Some(vec!["moves", "wave.json"])
        );
        for bad in [
            "",
            ".",
            "..",
            "./SKILL.md",
            "moves/../SKILL.md",
            "/SKILL.md",
            ".package.json",
            "moves/.hidden",
            "a\nb",
            "a\0b",
        ] {
            assert_eq!(parts_of(bad), None, "{bad:?}");
        }
        assert_eq!(parts_of(&"a".repeat(PATH_MOST + 1)), None);
    }

    /// The four groups' words, as the app shows where a skill came from.
    #[test]
    fn each_group_has_its_source_word() {
        assert_eq!(
            [
                Group::Core,
                Group::Installed,
                Group::MadeHere,
                Group::Pushed
            ]
            .map(source),
            ["built-in", "store", "made", "pushed"]
        );
    }
}
