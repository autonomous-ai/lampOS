use lamp_apps::staging::{MOST_STAGED, read_staged, staged_path};
use std::fs;
use std::os::unix::fs::{MetadataExt, symlink};
use std::path::Path;

fn rig() -> (tempfile::TempDir, std::path::PathBuf, u32) {
    let root = tempfile::tempdir().unwrap();
    let staging = root.path().join("staging");
    fs::create_dir(&staging).unwrap();
    let owner = fs::metadata(&staging).unwrap().uid();
    (root, staging, owner)
}

#[test]
fn only_a_plain_file_directly_under_the_staging_folder_is_a_path() {
    let (_root, staging, _) = rig();
    let ok = staging.join("pkg-1.tar");
    assert_eq!(staged_path(&staging, ok.to_str().unwrap()).unwrap(), ok);
    for bad in ["", "pkg.tar", "/etc/passwd", "/tmp/pkg.tar"] {
        assert!(staged_path(&staging, bad).is_err(), "{bad:?}");
    }
    let s = staging.display();
    for bad in [
        format!("{s}/../x.tar"),
        format!("{s}/sub/x.tar"),
        format!("{s}/.hidden"),
        format!("{s}/"),
        format!("{s}"),
        format!("{s}/a\nb"),
        format!("{s}/{}", "a".repeat(200)),
        format!("{s}//x.tar/.."),
    ] {
        assert!(staged_path(&staging, &bad).is_err(), "{bad:?}");
    }
}

#[test]
fn a_good_staged_file_is_read_whole() {
    let (_root, staging, owner) = rig();
    let path = staging.join("p.tar");
    fs::write(&path, b"package bytes").unwrap();
    assert_eq!(
        read_staged(&staging, path.to_str().unwrap(), owner).unwrap(),
        b"package bytes"
    );
}

#[test]
fn a_link_a_folder_a_pipe_a_hard_link_and_a_stranger_are_refused() {
    let (root, staging, owner) = rig();
    let real = root.path().join("real");
    fs::write(&real, b"x").unwrap();
    symlink(&real, staging.join("link.tar")).unwrap();
    fs::create_dir(staging.join("dir.tar")).unwrap();
    fs::write(staging.join("two.tar"), b"x").unwrap();
    fs::hard_link(staging.join("two.tar"), root.path().join("other-name")).unwrap();
    fs::write(staging.join("mine.tar"), b"x").unwrap();
    let fifo = staging.join("pipe.tar");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    for name in ["link.tar", "dir.tar", "two.tar", "pipe.tar", "missing.tar"] {
        let path = staging.join(name);
        assert!(
            read_staged(&staging, path.to_str().unwrap(), owner).is_err(),
            "{name}"
        );
    }
    let path = staging.join("mine.tar");
    assert!(
        read_staged(&staging, path.to_str().unwrap(), owner ^ 1).is_err(),
        "not lampos's"
    );
}

#[test]
fn a_file_over_the_cap_is_refused_without_reading_it() {
    let (_root, staging, owner) = rig();
    let path = staging.join("big.tar");
    let file = fs::File::create(&path).unwrap();
    file.set_len(MOST_STAGED + 1).unwrap();
    let error = read_staged(&staging, path.to_str().unwrap(), owner).unwrap_err();
    assert!(error.contains("big") || error.contains("size"), "{error}");
    let _ = Path::new("");
}
