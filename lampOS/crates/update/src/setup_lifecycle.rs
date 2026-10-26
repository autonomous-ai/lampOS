//! Leave the completed product Setup's phone grace period alone. This is
//! restart selection, never permission for a watched persistent part to stop.
use crate::{install, unit};
use std::{
    fs,
    io::{self, Read},
    os::unix::fs::OpenOptionsExt,
    path::Path,
};

const SETUP: &str = "lamp-setup.service";
const PRODUCT: &str = include_str!("../../../deploy/product/lamp-setup.service");
const UNIT_DIR: &str = "etc/systemd/system";
const MOST_BYTES: u64 = 16 * 1024;
const MOST_ENTRIES: usize = 128;
// Include higher-precedence control/transient/generated units as well as the
// ordinary product roots. An unfamiliar fragment or override disables this
// exception; this is not a partial implementation of systemd's merge rules.
const ROOTS: &[&str] = &[
    "etc/systemd/system.control",
    "run/systemd/system.control",
    "run/systemd/transient",
    "run/systemd/generator.early",
    UNIT_DIR,
    "etc/systemd/system.attached",
    "run/systemd/system",
    "run/systemd/system.attached",
    "run/systemd/generator",
    "usr/local/lib/systemd/system",
    "usr/lib/systemd/system",
    "lib/systemd/system",
    "run/systemd/generator.late",
];

// Do not follow local overrides through symlinks, including their directories.
// Missing paths are ordinary on a product; unreadable paths are uncertainty.
fn plain(root: &Path, relative: &Path) -> bool {
    let mut path = root.to_owned();
    for part in relative.components() {
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => return false,
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return true,
            Err(_) => return false,
        }
    }
    true
}

fn read(root: &Path, relative: &Path) -> Option<String> {
    if !plain(root, relative) {
        return None;
    }
    let flags =
        rustix::fs::OFlags::NONBLOCK | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NOCTTY;
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(flags.bits() as i32)
        .open(root.join(relative))
        .ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() > MOST_BYTES {
        return None;
    }
    let mut text = String::new();
    file.take(MOST_BYTES + 1).read_to_string(&mut text).ok()?;
    (text.len() as u64 <= MOST_BYTES).then_some(text)
}

pub(crate) fn completed(root: &Path, units: &Path, name: &str, before: &str, after: &str) -> bool {
    if name != SETUP
        || units != root.join(UNIT_DIR)
        || !unit::same(before, PRODUCT)
        || !unit::same(after, PRODUCT)
    {
        return false;
    }
    if !done(root)
        || !read(root, &Path::new(UNIT_DIR).join(SETUP))
            .is_some_and(|text| unit::same(&text, PRODUCT))
    {
        return false;
    }
    for base in ROOTS {
        // Merged-/usr products have this distribution alias. Its destination
        // is inspected separately; no custom override symlink is followed.
        if *base == "lib/systemd/system"
            && fs::read_link(root.join("lib"))
                .is_ok_and(|to| to == Path::new("usr/lib") || to == Path::new("/usr/lib"))
        {
            continue;
        }
        let fragment = Path::new(base).join(SETUP);
        if *base != UNIT_DIR {
            if !plain(root, &fragment) {
                return false;
            }
            match fs::symlink_metadata(root.join(&fragment)) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                _ => return false,
            }
        }
        for folder in ["service.d", "lamp-.service.d", "lamp-setup.service.d"] {
            let relative = Path::new(base).join(folder);
            if !plain(root, &relative) {
                return false;
            }
            let entries = match fs::read_dir(root.join(&relative)) {
                Ok(entries) => entries,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(_) => return false,
            };
            for (count, entry) in entries.enumerate() {
                if count >= MOST_ENTRIES {
                    return false;
                }
                let Ok(entry) = entry else { return false };
                if entry.path().extension().is_none_or(|ext| ext != "conf") {
                    continue;
                }
                // Only install's exact, unit-local exit-note hook is known
                // harmless. Conditions, resets and custom service arguments
                // in any other drop-in retain ordinary restart/watch rules.
                if *base != UNIT_DIR
                    || folder != "lamp-setup.service.d"
                    || entry.file_name() != "exit-note.conf"
                    || !read(root, &relative.join(entry.file_name()))
                        .is_some_and(|text| unit::same(&text, install::EXIT_NOTE))
                {
                    return false;
                }
            }
        }
    }
    // A reset can remove completion while the override directories are read.
    // Observe it again at the restart decision; this is not a reset lock.
    done(root)
}

pub(crate) fn done(root: &Path) -> bool {
    let marker = Path::new("var/lib/lampos-setup/done");
    plain(root, marker) && fs::symlink_metadata(root.join(marker)).is_ok_and(|meta| meta.is_file())
}

/// A fixed-property query, not a systemd unit parser. Loaded aliases may
/// survive filesystem changes; their drop-ins must not escape the file check.
pub(crate) fn manager_matches(text: &str) -> bool {
    if text.len() > MOST_BYTES as usize {
        return false;
    }
    let mut seen = std::collections::BTreeSet::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            return false;
        };
        if !seen.insert(key) {
            return false;
        }
        let matches = match key {
            "Id" | "Names" => value == SETUP,
            "LoadState" => value == "loaded",
            "FragmentPath" => value == "/etc/systemd/system/lamp-setup.service",
            "DropInPaths" => {
                value.is_empty()
                    || value == "/etc/systemd/system/lamp-setup.service.d/exit-note.conf"
            }
            _ => false,
        };
        if !matches {
            return false;
        }
    }
    seen.len() == 5
}

#[cfg(test)]
mod tests {
    use super::*;
    const PROPERTIES: &str = "Id=lamp-setup.service\nNames=lamp-setup.service\nLoadState=loaded\nFragmentPath=/etc/systemd/system/lamp-setup.service\nDropInPaths=\n";

    #[test]
    fn manager_identity_requires_the_exact_loaded_product_with_known_dropins() {
        assert!(manager_matches(PROPERTIES));
        assert!(manager_matches(&PROPERTIES.replace(
            "DropInPaths=",
            "DropInPaths=/etc/systemd/system/lamp-setup.service.d/exit-note.conf"
        )));
        for (old, new) in [
            ("Id=lamp-setup.service", "Id=bootstrap.service"),
            (
                "Names=lamp-setup.service",
                "Names=lamp-setup.service bootstrap.service",
            ),
            (
                "Names=lamp-setup.service",
                "Names=bootstrap.service lamp-setup.service",
            ),
            ("LoadState=loaded", "LoadState=not-found"),
            (
                "FragmentPath=/etc/systemd/system/lamp-setup.service",
                "FragmentPath=/custom/lamp-setup.service",
            ),
            (
                "DropInPaths=",
                "DropInPaths=/etc/systemd/system/bootstrap.service.d/override.conf",
            ),
            ("Names=lamp-setup.service", "Names=lamp-setup\\x2eservice"),
        ] {
            assert!(!manager_matches(&PROPERTIES.replace(old, new)), "{new}");
        }
    }

    #[test]
    fn incomplete_duplicate_or_malformed_manager_records_are_unknown() {
        assert!(!manager_matches(""));
        for line in PROPERTIES.lines() {
            assert!(!manager_matches(
                &PROPERTIES.replace(&format!("{line}\n"), "")
            ));
            assert!(!manager_matches(&format!("{PROPERTIES}{line}\n")));
        }
        assert!(!manager_matches(&format!("{PROPERTIES}Future=yes\n")));
        assert!(!manager_matches(&format!("{PROPERTIES}not a property\n")));
        assert!(!manager_matches(&format!("{PROPERTIES}\n{PROPERTIES}")));
    }
}
