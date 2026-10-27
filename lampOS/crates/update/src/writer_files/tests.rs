use super::*;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
    sync::mpsc,
    thread::{self, JoinHandle},
    time::{Duration, SystemTime},
};

fn private_root() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap()
}

fn write(root: &Path, relative: &str, bytes: &[u8]) -> PathBuf {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, bytes).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    path
}

#[test]
fn regular_bytes_and_all_existing_ancestor_identities_are_observed_without_changing_them() {
    let root = private_root();
    let path = write(root.path(), "a/b/setting", &[0, 255, 17, 42]);
    let observed = observe(root.path(), "a/b/setting", 4).unwrap();
    assert_eq!(observed.ancestors.len(), 3);
    for (identity, path) in observed.ancestors.iter().zip([
        root.path().to_owned(),
        root.path().join("a"),
        root.path().join("a/b"),
    ]) {
        assert_eq!(*identity, Identity::of(&fs::metadata(path).unwrap()));
    }
    let Entry::Present { bytes, identity } = observed.entry else {
        panic!("regular file was not observed");
    };
    assert_eq!(bytes, [0, 255, 17, 42]);
    assert_eq!(identity, Identity::of(&fs::metadata(&path).unwrap()));
    assert_eq!(identity.mode & 0o777, 0o640);
    let again = observe(root.path(), "a/b/setting", 4).unwrap();
    assert!(matches!(again.entry, Entry::Present { identity: next, .. } if next == identity));
    assert_eq!(fs::read(path).unwrap(), bytes);
}

#[test]
fn missing_intermediate_and_leaf_are_distinct_and_keep_only_existing_ancestors() {
    let root = private_root();
    fs::create_dir(root.path().join("a")).unwrap();
    let missing = observe(root.path(), "a/b/setting", 10).unwrap();
    assert_eq!(missing.ancestors.len(), 2);
    assert!(matches!(
        missing.entry,
        Entry::MissingAncestor { component: 1 }
    ));
    assert!(!root.path().join("a/b").exists());
    let first = observe(root.path(), "absent/unknown/setting", 10).unwrap();
    assert_eq!(first.ancestors.len(), 1);
    assert!(matches!(
        first.entry,
        Entry::MissingAncestor { component: 0 }
    ));
    fs::create_dir(root.path().join("a/b")).unwrap();
    let missing = observe(root.path(), "a/b/setting", 10).unwrap();
    assert_eq!(missing.ancestors.len(), 3);
    assert!(matches!(missing.entry, Entry::MissingLeaf));
    assert!(!root.path().join("a/b/setting").exists());
    write(root.path(), "a/b/setting", b"");
    let empty = observe(root.path(), "a/b/setting", 0).unwrap();
    assert!(matches!(empty.entry, Entry::Present { bytes, .. } if bytes.is_empty()));
}

#[test]
fn invalid_or_unbounded_relative_names_are_refused_before_opening_the_root() {
    let root = private_root();
    let absent_root = root.path().join("not-created");
    for path in [
        "",
        "/setting",
        "a//setting",
        "a/./setting",
        "a/../setting",
        "../setting",
        "a/",
        "a/\0setting",
    ] {
        assert_eq!(
            observe(&absent_root, path, 10).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
    for path in [
        "x".repeat(MOST_PATH_BYTES + 1),
        vec!["x"; MOST_COMPONENTS + 1].join("/"),
    ] {
        assert_eq!(
            observe(&absent_root, &path, 10).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
    assert!(!absent_root.exists());
}

#[test]
fn explicit_content_caps_and_the_hard_ceiling_are_enforced_without_truncating_success() {
    let root = private_root();
    let content = vec![b'x'; MOST_BYTES];
    write(root.path(), "setting", &content);
    assert!(
        matches!(observe(root.path(), "setting", MOST_BYTES).unwrap().entry, Entry::Present { bytes, .. } if bytes == content)
    );
    assert!(observe(root.path(), "setting", MOST_BYTES - 1).is_err());
    assert!(observe(root.path(), "setting", 0).is_err());
    assert_eq!(
        observe(root.path(), "setting", MOST_BYTES + 1)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
    assert_eq!(fs::read(root.path().join("setting")).unwrap(), content);
}

#[test]
fn no_follow_rejects_leaf_ancestor_and_root_links_without_reading_the_outside_witness() {
    let root = private_root();
    let outside = private_root();
    let content = b"outside private witness";
    let witness = write(outside.path(), "setting", content);
    symlink(&witness, root.path().join("leaf")).unwrap();
    symlink(outside.path(), root.path().join("ancestor")).unwrap();
    symlink(outside.path(), root.path().join("root-link")).unwrap();
    assert!(observe(root.path(), "leaf", 100).is_err());
    assert!(observe(root.path(), "ancestor/setting", 100).is_err());
    assert!(observe(&root.path().join("root-link"), "setting", 100).is_err());
    assert_eq!(fs::read(&witness).unwrap(), content);
    assert_eq!(fs::read_link(root.path().join("leaf")).unwrap(), witness);
    symlink(outside.path().join("absent"), root.path().join("dangling")).unwrap();
    assert!(observe(root.path(), "dangling", 100).is_err());
}

#[test]
fn a_hard_link_is_not_accepted_as_a_setting_and_directories_are_not_read() {
    let root = private_root();
    let outside = private_root();
    let witness = write(outside.path(), "witness", b"unchanged");
    fs::hard_link(&witness, root.path().join("setting")).unwrap();
    assert!(
        observe_inner(
            root.path(),
            "setting",
            100,
            || panic!("known hard-linked leaf reached open"),
            || panic!("known hard-linked leaf reached reading"),
        )
        .is_err()
    );
    fs::create_dir(root.path().join("directory")).unwrap();
    assert!(
        observe_inner(
            root.path(),
            "directory",
            100,
            || panic!("known nonregular leaf reached open"),
            || panic!("nonregular leaf reached the post-read hook")
        )
        .is_err()
    );
    assert_eq!(fs::read(witness).unwrap(), b"unchanged");
}

// A test regression that blocks opening/reading the FIFO must still be joined
// during unwinding. Opening and closing this local read/write endpoint supplies
// a writer and EOF; no production thread or unrelated FIFO is touched.
struct FifoWorker {
    path: PathBuf,
    worker: Option<JoinHandle<()>>,
}

impl Drop for FifoWorker {
    fn drop(&mut self) {
        let unblock = rustix::fs::open(
            &self.path,
            OFlags::RDWR | OFlags::NONBLOCK | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .ok();
        drop(unblock);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[test]
fn a_fifo_is_refused_without_waiting_for_a_writer_and_its_worker_is_always_joined() {
    let root = private_root();
    let fifo = root.path().join("setting");
    assert!(
        std::process::Command::new("mkfifo")
            .args(["-m", "600"])
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let (send, receive) = mpsc::channel();
    let directory = root.path().to_owned();
    let worker = thread::spawn(move || {
        let result = observe_inner(
            &directory,
            "setting",
            100,
            || panic!("known FIFO reached open"),
            || panic!("FIFO reached the post-read hook"),
        );
        let _ = send.send(result);
    });
    let cleanup = FifoWorker {
        path: fifo,
        worker: Some(worker),
    };
    let result = receive.recv_timeout(Duration::from_secs(2));
    // Unblock and join before any assertion, including the timeout path.
    drop(cleanup);
    assert!(
        result
            .expect("FIFO observation waited for a writer")
            .is_err()
    );
}

#[test]
fn descriptor_and_ancestor_metadata_changes_refuse_the_observation() {
    for change in 0..6 {
        let root = private_root();
        let file = write(root.path(), "a/setting", b"before");
        let old_time = SystemTime::UNIX_EPOCH + Duration::from_secs(12_345);
        let observed = observe_inner(
            root.path(),
            "a/setting",
            100,
            || {},
            || match change {
                0 => fs::write(&file, b"longer content").unwrap(),
                1 => fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap(),
                2 => File::open(&file).unwrap().set_modified(old_time).unwrap(),
                3 => fs::hard_link(&file, root.path().join("extra-link")).unwrap(),
                4 => File::open(root.path().join("a"))
                    .unwrap()
                    .set_modified(old_time)
                    .unwrap(),
                5 => {
                    fs::rename(root.path().join("a"), root.path().join("old-a")).unwrap();
                    write(root.path(), "a/setting", b"replacement");
                    File::open(root.path())
                        .unwrap()
                        .set_modified(old_time)
                        .unwrap();
                }
                _ => unreachable!(),
            },
        );
        let error = observed.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("changed"));
    }
}

#[test]
fn pre_open_inode_or_metadata_substitution_and_disappearance_are_not_read_as_the_original_file() {
    for change in 0..3 {
        let root = private_root();
        let file = write(root.path(), "setting", b"before");
        let replacement = write(root.path(), "replacement", b"before");
        let observed = observe_inner(
            root.path(),
            "setting",
            100,
            || match change {
                0 => fs::rename(&replacement, &file).unwrap(),
                1 => fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap(),
                2 => fs::remove_file(&file).unwrap(),
                _ => unreachable!(),
            },
            || panic!("a pre-open substitution reached file reading"),
        );
        let error = observed.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("changed before reading"));
    }
}

#[test]
fn errors_and_debug_do_not_expose_relative_names_or_file_contents() {
    let root = private_root();
    let relative = "private-person-value/private-file-value";
    let contents = b"private contents that must never enter Debug";
    write(root.path(), relative, contents);
    let observed = observe(root.path(), relative, 100).unwrap();
    let debug = format!("{observed:?}");
    assert!(!debug.contains("private-person-value"));
    assert!(!debug.contains("private-file-value"));
    assert!(!debug.contains("private contents"));
    let error = observe(root.path(), relative, 1).unwrap_err();
    let diagnostic = format!("{error:?}: {error}");
    assert!(!diagnostic.contains("private-person-value"));
    assert!(!diagnostic.contains("private-file-value"));
    assert!(!diagnostic.contains("private contents"));
}
