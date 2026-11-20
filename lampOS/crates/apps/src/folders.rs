//! `lamp-apps-manage folders`: the apps' three folders, made empty (#7415, #7920).
//!
//! Run by `lamp-apps-folders.service` (root, before the helper's own units,
//! which `Want` it and skip while the folders are absent). The folders are
//! not the product's tmpfiles: their switch-time run is inside
//! `lamp-update-apply.service`, which cannot write them on a lamp that already
//! has it. It is a unit of the apps' own part so a round can carry and add it
//! (a round carries binary-backed units only). It needs no switch: it makes
//! empty folders and nothing else.
use crate::layout::Layout;
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

/// The code and state folders, 0755, and the staging folder, 0700 and handed to
/// `lampos` by `give` (called once, with the staging folder). A folder that is
/// there has its mode put right. Nothing else is made, read or changed.
pub fn make(
    layout: &Layout,
    give: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<Vec<String>, String> {
    let mut said = Vec::new();
    for (path, mode) in [
        (&layout.code, 0o755),
        (&layout.state, 0o755),
        (&layout.staging, 0o700),
    ] {
        let there = path.is_dir();
        fs::create_dir_all(path).map_err(|error| format!("{}: {error}", path.display()))?;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .map_err(|error| format!("{}: {error}", path.display()))?;
        said.push(format!(
            "{}{}: {mode:04o}",
            path.display(),
            if there { "" } else { " made" }
        ));
    }
    give(&layout.staging)?;
    Ok(said)
}

/// `chown lampos:lampos PATH`: the program does the name lookup, so this
/// process reads no file for it. A fixed program, a fixed pair of names, no
/// environment.
pub fn give_to_lampos(path: &Path) -> Result<(), String> {
    let status = Command::new("/usr/bin/chown")
        .arg("lampos:lampos")
        .arg(path)
        .env_clear()
        .status()
        .map_err(|error| format!("chown: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("chown lampos:lampos {}: {status}", path.display()))
    }
}
