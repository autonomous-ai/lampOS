//! The build's own lampos SDK, with the pure-Python `websockets` it needs, on the lamp as ONE read-only copy
//! for every agent (#6507, #6071): a skill's script runs in whichever Python the engine has (`/usr/bin/python3`
//! has no `websockets`; Hermes's venv has both), so `from lampos import Lamp` must not depend on it.
//!
//! - **The files are this build's own**, embedded in lamp-update when it is built ([`FILES`]:
//!   `sdk/python/src/lampos` and `deploy/sdk-runtime/websockets`, a list a test holds to the two folders), so
//!   the SDK on a lamp is the one of the build that updated it, with no fetch at run time and nothing from an
//!   index.
//! - **Where:** [`DIR`], `/usr/local/lib/lampos/sdk-python`, root's, folders 0755 and files 0644. The Hermes
//!   namespace leaves `/usr` as it is, so a run reads it and cannot change it (`lamp-tasks` puts it on the run's
//!   `PYTHONPATH`, [`DIR`] as `lamp_tasks::skill_task::SDK_PYTHONPATH`).
//! - **Who writes it:** root's one-shot, as it writes Hermes's entry points ([`crate::hermes_install`]):
//!   [`write`] stages a copy beside [`DIR`] and renames it in. [`present`] says the folder holds exactly
//!   [`FILES`]; Hermes counts as missing, and is put there again, while it does not, so a round of a newer
//!   build has the SDK written again at its next one-shot.
use crate::records::write_whole;
use std::{
    collections::BTreeSet,
    fs, io,
    os::unix::fs::{DirBuilderExt, MetadataExt},
    path::{Path, PathBuf},
};

include!("sdk_tree_files.rs");

/// Where the one copy is, as the lamp names it.
pub const DIR: &str = "/usr/local/lib/lampos/sdk-python";

fn on(root: &Path, path: &str) -> PathBuf {
    root.join(path.trim_start_matches('/'))
}

/// Every plain file under `dir`, by its path in the folder; `None` for a link or a name that is not UTF-8.
fn found(dir: &Path) -> Option<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    let mut folders = vec![(dir.to_path_buf(), String::new())];
    while let Some((folder, prefix)) = folders.pop() {
        for entry in fs::read_dir(&folder).ok()? {
            let entry = entry.ok()?;
            let name = entry.file_name().into_string().ok()?;
            let path = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let meta = fs::symlink_metadata(entry.path()).ok()?;
            if meta.is_dir() {
                folders.push((entry.path(), path));
            } else if meta.is_file() {
                names.insert(path);
            } else {
                return None;
            }
        }
    }
    Some(names)
}

/// Whether [`DIR`] under `root` holds exactly [`FILES`]: every file there with these bytes, root's alone to
/// write (not group- or world-writable), and no other file or link.
pub fn present(root: &Path) -> bool {
    let dir = on(root, DIR);
    let Some(names) = found(&dir) else {
        return false;
    };
    if names != FILES.iter().map(|(name, _)| (*name).to_owned()).collect() {
        return false;
    }
    FILES.iter().all(|(name, bytes)| {
        let place = dir.join(name);
        fs::symlink_metadata(&place).is_ok_and(|meta| meta.is_file() && meta.mode() & 0o022 == 0)
            && fs::read(&place).is_ok_and(|read| read == *bytes)
    })
}

/// [`FILES`] written to a new folder beside [`DIR`] under `root` (folders 0755, files 0644), then renamed in
/// place of the old one, which is removed. The folder above [`DIR`] must be there.
pub fn write(root: &Path) -> Result<(), String> {
    let dir = on(root, DIR);
    let (staged, old) = (
        dir.with_file_name("sdk-python.new"),
        dir.with_file_name("sdk-python.old"),
    );
    let failed = |what: &str, error: io::Error| format!("{what}: {error}");
    let parent = dir.parent().ok_or("no folder above the SDK's")?;
    if !parent.is_dir() {
        return Err(format!("{}: no such folder", parent.display()));
    }
    for leftover in [&staged, &old] {
        if fs::symlink_metadata(leftover).is_ok() {
            fs::remove_dir_all(leftover).map_err(|e| failed(&leftover.display().to_string(), e))?;
        }
    }
    for (name, bytes) in FILES {
        let place = staged.join(name);
        let folder = place.parent().expect("a file has a folder");
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o755)
            .create(folder)
            .map_err(|e| failed(&folder.display().to_string(), e))?;
        write_whole(&place, bytes, 0o644).map_err(|e| failed(&place.display().to_string(), e))?;
    }
    if fs::symlink_metadata(&dir).is_ok() {
        fs::rename(&dir, &old).map_err(|e| failed(&dir.display().to_string(), e))?;
    }
    fs::rename(&staged, &dir).map_err(|e| failed(&dir.display().to_string(), e))?;
    let _ = fs::remove_dir_all(&old);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lamp(tag: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("lamp-update-sdk-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(on(&root, "/usr/local/lib/lampos")).unwrap();
        root
    }

    /// The embedded list is the folders' files, so a file added to the SDK or the vendored websockets without
    /// making `sdk_tree_files.rs` again fails here.
    #[test]
    fn the_embedded_list_is_every_file_of_both_folders() {
        let here = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut listed = BTreeSet::new();
        for (prefix, folder) in [
            ("lampos", "../../sdk/python/src/lampos"),
            ("websockets", "../../deploy/sdk-runtime/websockets"),
        ] {
            let all = found(&here.join(folder)).unwrap();
            let all: BTreeSet<String> = all
                .into_iter()
                .filter(|name| !name.contains("__pycache__"))
                .map(|name| format!("{prefix}/{name}"))
                .collect();
            listed.extend(all);
        }
        let embedded: BTreeSet<String> = FILES.iter().map(|(name, _)| (*name).to_owned()).collect();
        assert_eq!(
            embedded, listed,
            "make crates/update/src/sdk_tree_files.rs again"
        );
        // The SDK's own entry points are in it.
        for name in [
            "lampos/__init__.py",
            "lampos/client.py",
            "websockets/asyncio/client.py",
        ] {
            assert!(embedded.contains(name), "{name}");
        }
    }

    /// The folder lamp-tasks puts on a run's `PYTHONPATH` (`skill_task::SDK_PYTHONPATH`) is this one.
    #[test]
    fn the_folder_is_the_one_a_runs_pythonpath_names() {
        assert_eq!(DIR, "/usr/local/lib/lampos/sdk-python");
    }

    #[test]
    fn the_sdk_is_written_whole_read_only_and_put_back_when_it_differs() {
        let root = lamp("write");
        assert!(!present(&root));
        write(&root).unwrap();
        assert!(present(&root));
        let dir = on(&root, DIR);
        let client = dir.join("lampos/client.py");
        let mode = |path: &Path| fs::metadata(path).unwrap().mode() & 0o777;
        assert_eq!(mode(&client), 0o644);
        assert_eq!(mode(&dir.join("lampos")), 0o755);
        // No staging or old copy is left.
        assert!(!dir.with_file_name("sdk-python.new").exists());
        assert!(!dir.with_file_name("sdk-python.old").exists());
        // A file changed, one added, one writable by others, a link: each is "not there".
        let same = fs::read(&client).unwrap();
        fs::write(&client, "changed").unwrap();
        assert!(!present(&root));
        write(&root).unwrap();
        assert_eq!(fs::read(&client).unwrap(), same);
        fs::write(dir.join("extra.py"), "x").unwrap();
        assert!(!present(&root));
        write(&root).unwrap();
        assert!(!dir.join("extra.py").exists() && present(&root));
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&client, fs::Permissions::from_mode(0o666)).unwrap();
        assert!(!present(&root));
        write(&root).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", dir.join("lampos/link")).unwrap();
        assert!(!present(&root));
        write(&root).unwrap();
        assert!(present(&root));
        // Written again with a leftover staging folder in the way.
        fs::create_dir_all(dir.with_file_name("sdk-python.new/stale")).unwrap();
        write(&root).unwrap();
        assert!(present(&root));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_lamp_with_no_folder_above_is_an_error_and_nothing_is_made() {
        let root =
            std::env::temp_dir().join(format!("lamp-update-sdk-bare-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        assert!(write(&root).unwrap_err().contains("no such folder"));
        assert!(!present(&root));
        let _ = fs::remove_dir_all(&root);
    }
}
