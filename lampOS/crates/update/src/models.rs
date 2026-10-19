//! The models a build carries (#1680, docs/updates.md). Off unless
//! `LAMP_UPDATE_MODELS=on`.
//!
//! A model is fetched into the store, `/opt/lampOS/models/<sha256>`, only
//! when no file of that SHA-256 is already where it is needed, and checked
//! before it is given its name. A model under a part's `current/` goes into
//! the part's new folder beside its binary, so the part's link switches it.
//! Any other model's path becomes a link into the store, swapped at the
//! switch beside the parts' links, journalled, and put back in reverse.
//! A store entry is kept while the build the lamp runs or the one before
//! names it, or a model path links to it now.
use crate::{
    manifest::{MODELS_ROOT, Model, STORE},
    records::{replace_link, sync_parent},
    round::{Config, file_sha256},
    unit,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs, io, path::PathBuf};

/// The switch, in lamp-update.service's environment: `on`, or off.
pub const SWITCH: &str = "LAMP_UPDATE_MODELS";

/// Whether the switch is on, from its value.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// A model's path as it was before a switch, to be put back.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelBack {
    /// As the lamp names it: `/opt/lampOS/eyes/models/x.onnx`.
    pub path: String,
    /// It was this SHA-256 (a link into the store, or a file whose bytes
    /// the store now also holds). None: it was not there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// It was a file, not a link: it is put back as a file.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub file: bool,
}

/// A model at a path of its own that a switch changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placing {
    pub model: Model,
    pub back: ModelBack,
}

/// Whether a model lives in a part's folder: its path has a `current`
/// part. Such a model goes beside the binary, never to a link of its own.
pub fn in_a_part(path: &str) -> bool {
    path.split('/').any(|part| part == unit::CURRENT)
}

/// The model's name in a part's folder, when its path is the part's
/// `<builds>current/<name>`.
pub fn name_in<'m>(model: &'m Model, builds: &str) -> Option<&'m str> {
    model
        .path
        .strip_prefix(&unit::current(builds))?
        .strip_prefix('/')
        .filter(|name| !name.contains('/'))
}

/// A store entry, as the lamp names it.
pub fn store_path(sha256: &str) -> String {
    format!("{STORE}{sha256}")
}

/// The link's target for a model's path: relative, so it is the same under
/// any root (`../../models/<sha256>`).
pub fn target(path: &str, sha256: &str) -> String {
    let below = path.strip_prefix(MODELS_ROOT).unwrap_or(path);
    let depth = below.matches('/').count();
    format!("{}models/{sha256}", "../".repeat(depth))
}

/// The SHA-256 the store link at `path` names, when it is one of ours.
fn linked_sha(config: &Config, path: &str) -> Option<String> {
    let now = fs::read_link(config.on(path)).ok()?;
    let now = now.to_str()?;
    let sha = now.rsplit('/').next()?;
    (sha.len() == 64 && now == target(path, sha)).then(|| sha.to_owned())
}

/// What a model at a path of its own needs: nothing (it is there), a switch
/// ([`Placing`]), or to be left as the lamp's own (said, as install says it).
pub enum Need {
    There,
    Switch(Placing),
    LampsOwn,
}

/// Whether a model at a path of its own is there, and if not, how it was.
/// `placed` is install's record of the path: the SHA-256 it placed.
pub fn need(config: &Config, model: &Model, placed: Option<&String>) -> Need {
    let at = config.on(&model.path);
    if let Some(sha) = linked_sha(config, &model.path) {
        if sha == model.sha256 && file_sha256(&at).is_ok_and(|now| now == sha) {
            return Need::There;
        }
        return Need::Switch(Placing {
            model: model.clone(),
            back: ModelBack {
                path: model.path.clone(),
                sha256: Some(sha),
                file: false,
            },
        });
    }
    let is_link = fs::symlink_metadata(&at).is_ok_and(|meta| meta.file_type().is_symlink());
    match file_sha256(&at) {
        Ok(sha) if sha == model.sha256 => Need::There,
        // A file install placed: switched, its bytes kept in the store.
        Ok(sha) if !is_link && placed == Some(&sha) => Need::Switch(Placing {
            model: model.clone(),
            back: ModelBack {
                path: model.path.clone(),
                sha256: Some(sha),
                file: true,
            },
        }),
        Ok(_) => Need::LampsOwn,
        Err(_) if is_link => Need::LampsOwn,
        Err(_) => Need::Switch(Placing {
            model: model.clone(),
            back: ModelBack {
                path: model.path.clone(),
                sha256: None,
                file: false,
            },
        }),
    }
}

/// Whether a store entry of this SHA-256 is there and whole.
pub fn stored(config: &Config, sha256: &str) -> bool {
    file_sha256(&config.on(&store_path(sha256))).is_ok_and(|sha| sha == sha256)
}

/// Whether a unit reads a model at this path: it names the path, or the
/// folder it is in as a folder.
pub fn reads(unit_text: &str, path: &str) -> bool {
    let folder = path.rsplit_once('/').map_or(path, |(folder, _)| folder);
    unit_text.contains(path)
        || unit_text.match_indices(folder).any(|(at, _)| {
            // The folder itself, with or without its slash: not a file in it.
            let after = &unit_text[at + folder.len()..];
            let after = after.strip_prefix('/').unwrap_or(after);
            after
                .chars()
                .next()
                .is_none_or(|next| next.is_whitespace() || next == '"')
        })
}

/// The path pointed at its model in the store, replaced whole. A file
/// install placed is first kept in the store by its bytes, for the way back.
pub fn switch(config: &Config, placing: &Placing) -> io::Result<()> {
    let at = config.on(&placing.model.path);
    if placing.back.file
        && let Some(sha) = &placing.back.sha256
    {
        let keep = config.on(&store_path(sha));
        if !keep.exists() {
            make_store(config)?;
            fs::hard_link(&at, &keep).or_else(|_| fs::copy(&at, &keep).map(|_| ()))?;
            sync_parent(&keep)?;
        }
    }
    if let Some(parent) = at.parent() {
        fs::create_dir_all(parent)?;
    }
    replace_link(&at, &target(&placing.model.path, &placing.model.sha256))
}

/// A path as it was before a switch.
pub fn put_back(config: &Config, back: &ModelBack) -> io::Result<()> {
    let at = config.on(&back.path);
    match (&back.sha256, back.file) {
        (None, _) => match fs::remove_file(&at) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => sync_parent(&at),
        },
        (Some(sha), false) => replace_link(&at, &target(&back.path, sha)),
        (Some(sha), true) => {
            if fs::symlink_metadata(&at).is_ok_and(|meta| meta.file_type().is_file()) {
                return Ok(());
            }
            let name = at
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let beside = at.with_file_name(format!(".{name}.lamp-update"));
            let _ = fs::remove_file(&beside);
            let kept = config.on(&store_path(sha));
            fs::hard_link(&kept, &beside).or_else(|_| fs::copy(&kept, &beside).map(|_| ()))?;
            fs::rename(&beside, &at)?;
            sync_parent(&at)
        }
    }
}

/// Whether a path is as it was before a switch: then the switch never moved
/// it, and nothing started on its new model.
pub fn is_back(config: &Config, back: &ModelBack) -> bool {
    let at = config.on(&back.path);
    match (&back.sha256, back.file) {
        (None, _) => fs::symlink_metadata(&at).is_err(),
        (Some(sha), false) => linked_sha(config, &back.path).as_ref() == Some(sha),
        (Some(sha), true) => {
            fs::symlink_metadata(&at).is_ok_and(|meta| meta.file_type().is_file())
                && file_sha256(&at).is_ok_and(|now| &now == sha)
        }
    }
}

/// The store entries a model path under [`MODELS_ROOT`] links to now,
/// whatever `state.json` says: prune keeps them, so no live link is left
/// pointing at nothing. The store and links to folders are not walked.
pub fn linked_now(config: &Config) -> BTreeSet<String> {
    fn walk(config: &Config, lamp: &str, found: &mut BTreeSet<String>, depth: usize) {
        let Ok(entries) = fs::read_dir(config.on(lamp)) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = format!("{lamp}{name}");
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                if let Some(sha) = linked_sha(config, &path) {
                    found.insert(sha);
                }
            } else if kind.is_dir() && depth > 0 && format!("{path}/") != STORE {
                walk(config, &format!("{path}/"), found, depth - 1);
            }
        }
    }
    let mut found = BTreeSet::new();
    // A model's path is short (manifest::is_model_path): a bound, so a
    // deep tree beside the builds is never all read.
    walk(config, MODELS_ROOT, &mut found, 16);
    found
}

pub fn make_store(config: &Config) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o755)
        .create(config.on(STORE))
}

/// Every store entry not in `kept`, and anything a fetch left, removed.
/// What is removed, as the lamp names it.
pub fn prune(config: &Config, kept: &BTreeSet<String>) -> Vec<Result<String, String>> {
    let store: PathBuf = config.on(STORE);
    let Ok(entries) = fs::read_dir(&store) else {
        return Vec::new();
    };
    let mut done = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let ours = (name.len() == 64 && crate::manifest::is_lower_hex(&name))
            || (name.starts_with('.')
                && (name.ends_with(".fetching") || name.ends_with(".archive")));
        if !ours || kept.contains(&name) {
            continue;
        }
        let path = store_path(&name);
        done.push(match fs::remove_file(entry.path()) {
            Ok(()) => Ok(path),
            Err(error) => Err(format!("{path} cannot be removed: {error}")),
        });
    }
    // Empty: gone too, so a lamp that keeps no model has no store.
    let _ = fs::remove_dir(&store);
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_into_the_store_is_relative_and_a_unit_reads_by_path_or_folder() {
        assert_eq!(
            target("/opt/lampOS/eyes/models/face.onnx", "ab"),
            "../../models/ab"
        );
        assert_eq!(target("/opt/lampOS/face.onnx", "ab"), "models/ab");
        let name = "ExecStart=/opt/lampOS/name/current/lamp-name --models /opt/lampOS/name/models --least 0.9";
        assert!(reads(name, "/opt/lampOS/name/models/name.onnx"));
        assert!(!reads(name, "/opt/lampOS/name/models-old/name.onnx"));
        let eyes = "ExecStart=x --model /opt/lampOS/eyes/models/face.onnx";
        assert!(reads(eyes, "/opt/lampOS/eyes/models/face.onnx"));
        assert!(!reads(eyes, "/opt/lampOS/eyes/models/other.onnx"));
        assert!(in_a_part("/opt/lampOS/voice/current/smart-turn.onnx"));
        assert!(!in_a_part("/opt/lampOS/eyes/models/face.onnx"));
    }
}
