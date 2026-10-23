//! Recover interrupted uploads before Cloud begins accepting account traffic.
//! The configured folder is dedicated to transient photos. Every root file
//! or link is removed, including older temporary names; no link is followed
//! and unexpected directories are preserved and fail uploads closed.
use super::Place;
use rustix::fs::{self, AtFlags, FileType, Mode, OFlags};
use std::{
    fs::File,
    io,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Component, Path},
};

/// A bounded startup scan, not a background or per-upload maintenance loop.
const MOST_ENTRIES: usize = 4_096;

/// Counts only: never expose a person's name, the file name or its path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub missing: bool,
    pub scanned: usize,
    pub removed_files: usize,
    pub removed_links: usize,
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Walk from the filesystem root without following a symlink at any level.
fn directory(path: &Path) -> io::Result<File> {
    if !path.is_absolute() || path.parent().is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "photo cleanup needs an absolute non-root directory",
        ));
    }
    let mut directory = File::open("/")?;
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                directory = fs::openat(
                    &directory,
                    name,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                )?
                .into();
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "photo cleanup directory has a dot component",
                ));
            }
        }
    }
    if directory.metadata()?.permissions().mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "photo cleanup directory is not private",
        ));
    }
    Ok(directory)
}

/// Sweep before starting MQTT/account tasks. A missing folder has no leftovers
/// and may still be created later by Eyes, preserving older installations.
/// Every other error must disable uploads for this Cloud invocation; partial
/// work is never reported as a completed sweep. No file contents are read.
pub fn startup(place: &Place) -> io::Result<Summary> {
    sweep(&place.dir, MOST_ENTRIES)
}

fn sweep(path: &Path, limit: usize) -> io::Result<Summary> {
    let directory = match directory(path) {
        Ok(directory) => directory,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(Summary {
                missing: true,
                ..Summary::default()
            });
        }
        Err(error) => return Err(error),
    };
    let identity = directory.metadata()?;
    let mut summary = Summary::default();
    let mut candidates = Vec::new();
    for entry in fs::Dir::read_from(&directory).map_err(io::Error::from)? {
        let entry = entry.map_err(io::Error::from)?;
        let name = entry.file_name();
        if matches!(name.to_bytes(), b"." | b"..") {
            continue;
        }
        if summary.scanned >= limit {
            return Err(invalid("photo cleanup entry bound exceeded"));
        }
        summary.scanned += 1;
        let stat = match fs::statat(&directory, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) => stat,
            Err(error) if error == rustix::io::Errno::NOENT => continue,
            Err(error) => return Err(error.into()),
        };
        let kind = FileType::from_raw_mode(stat.st_mode);
        if kind == FileType::Directory {
            // Never recurse into or remove a directory. Its contents may be
            // photos too, so preserve it and refuse to claim a complete sweep.
            return Err(invalid("photo cleanup found an unexpected directory"));
        }
        candidates.push((name.to_owned(), kind));
    }
    // Gather first: deleting while readdir is advancing may change its
    // cookies on some filesystems. The entry cap also bounds this list.
    for (name, kind) in candidates {
        match fs::unlinkat(&directory, name.as_c_str(), AtFlags::empty()) {
            Ok(()) if kind == FileType::Symlink => summary.removed_links += 1,
            Ok(()) => summary.removed_files += 1,
            Err(error) if error == rustix::io::Errno::NOENT => {}
            Err(error) => return Err(error.into()),
        }
    }
    // A fresh independent directory stream must see no root leftovers.
    // Concurrent additions or a partial sweep disable uploads, not a false
    // claim that the dedicated folder has been cleared.
    for entry in fs::Dir::read_from(&directory).map_err(io::Error::from)? {
        let entry = entry.map_err(io::Error::from)?;
        if !matches!(entry.file_name().to_bytes(), b"." | b"..") {
            return Err(invalid("photo cleanup still has entries"));
        }
    }
    // Do not enable path-based uploads if the configured directory was
    // replaced while this descriptor-relative sweep was working.
    let current = self::directory(path)?.metadata()?;
    if (identity.dev(), identity.ino()) != (current.dev(), current.ino()) {
        return Err(invalid("photo cleanup directory changed"));
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::{self, DirBuilder},
        os::unix::fs::{DirBuilderExt, symlink},
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
        time::Instant,
    };

    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            // On macOS /var and /tmp may themselves be symlinks; tests use
            // the real directory so the same nofollow policy runs on both OSes.
            let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
                "photo-sweep-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            DirBuilder::new().mode(0o700).create(&root).unwrap();
            Self(root)
        }

        fn place(&self) -> Place {
            Place {
                dir: self.0.clone(),
                socket: self.0.join("unused.sock"),
            }
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn startup_sweep_removes_every_root_file_including_old_temporary_names() {
        let scratch = Scratch::new();
        for stamp in 0..32 {
            fs::write(scratch.0.join(format!("app-{stamp}.jpg")), b"leftover").unwrap();
        }
        for name in ["leftover.jpg", ".part", "old-temporary-file"] {
            fs::write(scratch.0.join(name), b"leftover").unwrap();
        }
        let started = Instant::now();
        let first = startup(&scratch.place()).unwrap();
        let first_us = started.elapsed().as_micros();
        assert_eq!(
            first,
            Summary {
                scanned: 35,
                removed_files: 35,
                ..Summary::default()
            }
        );
        let repeated = Instant::now();
        let second = startup(&scratch.place()).unwrap();
        eprintln!(
            "photo sweep: files=35 first_us={first_us} repeated_us={}",
            repeated.elapsed().as_micros()
        );
        assert_eq!(second.removed_files + second.removed_links, 0);
        assert_eq!(second.scanned, 0);
        assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 0);
    }

    #[test]
    fn startup_sweep_unlinks_links_without_touching_outside_files_or_directories() {
        let scratch = Scratch::new();
        let outside = Scratch::new();
        fs::write(outside.0.join("kept.jpg"), b"outside").unwrap();
        symlink(outside.0.join("kept.jpg"), scratch.0.join("app-1.jpg")).unwrap();
        symlink(&outside.0, scratch.0.join("app-2.jpg")).unwrap();
        let summary = startup(&scratch.place()).unwrap();
        assert_eq!(summary.removed_files, 0);
        assert_eq!(summary.removed_links, 2);
        assert_eq!(fs::read(outside.0.join("kept.jpg")).unwrap(), b"outside");
        assert!(outside.0.is_dir());
    }

    #[test]
    fn startup_sweep_never_follows_the_configured_directory_or_an_ancestor_link() {
        let scratch = Scratch::new();
        let outside = Scratch::new();
        fs::write(outside.0.join("app-1.jpg"), b"outside").unwrap();
        symlink(&outside.0, scratch.0.join("linked")).unwrap();
        for dir in [scratch.0.join("linked"), scratch.0.join("linked/child")] {
            let mut place = scratch.place();
            place.dir = dir;
            assert!(startup(&place).is_err());
        }
        assert_eq!(fs::read(outside.0.join("app-1.jpg")).unwrap(), b"outside");
    }

    #[test]
    fn startup_sweep_preserves_unexpected_subfolders_and_fails_closed_without_recursion() {
        let scratch = Scratch::new();
        fs::create_dir(scratch.0.join("unexpected")).unwrap();
        fs::write(scratch.0.join("unexpected/leftover.jpg"), b"nested").unwrap();
        assert_eq!(
            startup(&scratch.place()).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            fs::read(scratch.0.join("unexpected/leftover.jpg")).unwrap(),
            b"nested"
        );
    }

    #[test]
    fn startup_sweep_missing_folder_is_a_noop_without_creating_it() {
        let scratch = Scratch::new();
        let mut place = scratch.place();
        place.dir.push("not-created");
        assert_eq!(
            startup(&place).unwrap(),
            Summary {
                missing: true,
                ..Summary::default()
            }
        );
        assert!(!place.dir.exists());
    }

    #[test]
    fn startup_sweep_refuses_nonprivate_or_invalid_roots_before_removing_files() {
        let scratch = Scratch::new();
        fs::write(scratch.0.join("app-1.jpg"), b"leftover").unwrap();
        fs::set_permissions(&scratch.0, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            startup(&scratch.place()).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(scratch.0.join("app-1.jpg").exists());
        for dir in [PathBuf::from("relative"), PathBuf::from("/")] {
            let mut place = scratch.place();
            place.dir = dir;
            assert_eq!(
                startup(&place).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
        }
    }

    #[test]
    fn startup_sweep_stops_at_its_entry_bound_without_recursing() {
        let scratch = Scratch::new();
        for name in ["one", "two", "three"] {
            fs::write(scratch.0.join(name), b"unrelated").unwrap();
        }
        assert_eq!(
            sweep(&scratch.0, 2).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 3);
    }
}
