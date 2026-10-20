use lamp_surround::atomic::{self, DirectoryLock};
use std::{fs, os::unix::fs::PermissionsExt};

fn temp_root() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(fs::canonicalize(std::env::temp_dir()).unwrap())
        .unwrap()
}

#[test]
fn replacement_is_whole_private_and_bounded() {
    let root = temp_root();
    let _lock = DirectoryLock::acquire(root.path()).unwrap();
    let path = root.path().join("state.json");
    assert_eq!(atomic::read(&path, 1024).unwrap(), None);
    atomic::write(&path, b"old", 1024).unwrap();
    atomic::write(&path, b"new record", 1024).unwrap();
    assert_eq!(atomic::read(&path, 1024).unwrap().unwrap(), b"new record");
    assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o077, 0);
    assert!(atomic::read(&path, 2).is_err());
    assert!(atomic::write(&path, b"oversize", 2).is_err());
    assert_eq!(atomic::read(&path, 1024).unwrap().unwrap(), b"new record");
}

#[test]
fn a_partial_uncommitted_temp_does_not_replace_the_journal() {
    let root = temp_root();
    let path = root.path().join("state.json");
    atomic::write(&path, b"committed", 1024).unwrap();
    let pending = root.path().join(".state.json.pending");
    fs::write(&pending, b"cut off next state").unwrap();
    fs::set_permissions(&pending, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(atomic::read(&path, 1024).unwrap().unwrap(), b"committed");
    atomic::write(&path, b"next", 1024).unwrap();
    assert_eq!(atomic::read(&path, 1024).unwrap().unwrap(), b"next");
    assert!(!pending.exists());
}

#[test]
fn failure_before_publication_preserves_the_committed_record() {
    let root = temp_root();
    let path = root.path().join("state.json");
    atomic::write(&path, b"committed", 1024).unwrap();
    fs::create_dir(root.path().join(".state.json.pending")).unwrap();
    assert!(atomic::write(&path, b"not acknowledged", 1024).is_err());
    assert_eq!(atomic::read(&path, 1024).unwrap().unwrap(), b"committed");
}

#[test]
fn leaf_and_ancestor_symlinks_are_refused_without_touching_their_targets() {
    let root = temp_root();
    let outside = temp_root();
    let target = outside.path().join("outside.json");
    atomic::write(&target, b"keep", 1024).unwrap();
    let path = root.path().join("state.json");
    std::os::unix::fs::symlink(&target, &path).unwrap();
    assert!(atomic::read(&path, 1024).is_err());
    assert!(atomic::write(&path, b"bad", 1024).is_err());
    assert!(atomic::remove(&path).is_err());
    let link = root.path().join("linked");
    std::os::unix::fs::symlink(outside.path(), &link).unwrap();
    assert!(atomic::read(&link.join("outside.json"), 1024).is_err());
    assert!(DirectoryLock::acquire(&link).is_err());
    assert_eq!(fs::read(target).unwrap(), b"keep");
}

#[test]
fn linked_temp_cannot_truncate_any_record() {
    let root = temp_root();
    let path = root.path().join("state.json");
    atomic::write(&path, b"keep", 1024).unwrap();
    fs::hard_link(&path, root.path().join(".other.json.pending")).unwrap();
    assert!(atomic::write(&root.path().join("other.json"), b"bad", 1024).is_err());
    assert_eq!(fs::read(path).unwrap(), b"keep");
}

#[test]
fn exposed_files_and_directories_are_not_silently_repaired() {
    let root = temp_root();
    let path = root.path().join("state.json");
    fs::write(&path, b"exposed").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(atomic::read(&path, 1024).is_err());
    assert!(atomic::write(&path, b"replacement", 1024).is_err());
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(DirectoryLock::acquire(root.path()).is_err());
}

#[test]
fn only_one_writer_holds_the_directory_until_drop() {
    let root = temp_root();
    let lock = DirectoryLock::acquire(root.path()).unwrap();
    assert!(DirectoryLock::acquire(root.path()).is_err());
    drop(lock);
    assert!(DirectoryLock::acquire(root.path()).is_ok());
}

#[test]
fn remove_does_not_remove_other_files() {
    let root = temp_root();
    let path = root.path().join("state.json");
    atomic::write(&path, b"remove", 1024).unwrap();
    atomic::write(&root.path().join("other.json"), b"keep", 1024).unwrap();
    atomic::remove(&path).unwrap();
    atomic::remove(&path).unwrap();
    assert_eq!(fs::read(root.path().join("other.json")).unwrap(), b"keep");
}

#[test]
fn relative_and_parent_paths_are_refused() {
    assert!(atomic::read(std::path::Path::new("state.json"), 1024).is_err());
    let root = temp_root();
    let escaped = root.path().join("../state.json");
    assert!(atomic::write(&escaped, b"bad", 1024).is_err());
    let child = root.path().join("records");
    atomic::ensure_private_dir(&child).unwrap();
    atomic::ensure_private_dir(&child).unwrap();
    assert_eq!(
        fs::metadata(child).unwrap().permissions().mode() & 0o777,
        0o700
    );
}
