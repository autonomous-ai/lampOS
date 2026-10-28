//! When the settings writer may be refreshed (#3689, docs/updates.md,
//! "When the writer may be refreshed").
//!
//! **Unwired:** no command calls [`ready`] yet. The refresh copies the
//! running lamp-update to the writer's fixed path. That is safe only once
//! lamp-update's own switch is settled: `lamp-update-back`, the copy about to
//! be replaced, is what puts an unconfirmed lamp-update back. So the refresh
//! waits until no `self.json` is there, and copies only the build that
//! `update/current` names, the one confirmed, which must be the one running.
use crate::{
    own::{self, BINARY, CURRENT, FOLDER},
    records::Lock,
};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Whether the writer may be refreshed from `running`, the bytes of the
/// lamp-update that runs: no switch of lamp-update's own waiting in
/// `records`, and `update/current` under `root` naming a build whose
/// lamp-update holds exactly those bytes. Why not, when not.
///
/// `_held` is the records' lock (`Records::lock` of `records`), which the
/// caller holds from here until the copy is placed: no switch of its own can
/// begin in between, so the answer still holds when it is acted on.
pub fn ready(root: &Path, records: &Path, _held: &Lock, running: &[u8]) -> Result<(), String> {
    match own::pending(records) {
        Ok(None) => {}
        Ok(Some(_)) => {
            return Err("a switch of lamp-update's own waits to be confirmed".to_owned());
        }
        Err(error) => {
            return Err(format!(
                "self.json cannot be read ({error}): its switch is not settled"
            ));
        }
    }
    let link = on_root(root, CURRENT);
    let build = fs::read_link(&link).map_err(|error| format!("{CURRENT}: {error}"))?;
    let build = build
        .to_str()
        .filter(|name| is_build(name))
        .ok_or_else(|| format!("{CURRENT} does not name a build: {}", build.display()))?;
    let binary = on_root(root, FOLDER).join(build).join(BINARY);
    let named = match fs::symlink_metadata(&binary) {
        Ok(metadata) if metadata.is_file() => {
            fs::read(&binary).map_err(|error| format!("{}: {error}", binary.display()))?
        }
        Ok(_) => return Err(format!("{} is not a regular file", binary.display())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(format!("{} is not there", binary.display()));
        }
        Err(error) => return Err(format!("{}: {error}", binary.display())),
    };
    if named != running {
        return Err(format!(
            "the lamp-update running is not the build {CURRENT} names ({})",
            &build[..12]
        ));
    }
    Ok(())
}

fn is_build(name: &str) -> bool {
    name.len() == 64 && name.bytes().all(|b| b.is_ascii_hexdigit())
}

fn on_root(root: &Path, path: &str) -> PathBuf {
    root.join(path.trim_start_matches('/'))
}

#[cfg(test)]
mod tests;
