use super::*;
use std::{cell::RefCell, os::unix::fs::symlink};

fn folder() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o7777
}

fn write(path: &Path, bytes: &[u8], mode: u32) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

/// A probe that answers as a good copy would, and notes what it ran.
fn good(ran: &RefCell<Vec<PathBuf>>) -> impl FnMut(&Path) -> Result<String, String> + '_ {
    |path: &Path| {
        ran.borrow_mut().push(path.to_owned());
        Ok(format!(
            "{}\n",
            answer(&sha256_hex(&fs::read(path).unwrap()))
        ))
    }
}

fn names(folder: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_new_copy_is_run_beside_and_then_renamed_in_executable() {
    let folder = folder();
    let path = folder.path().join("lamp-update-back");
    let ran = RefCell::new(Vec::new());
    let placed = place(&path, b"new", None, &mut good(&ran)).unwrap();
    assert_eq!(
        placed,
        Placed::Replaced {
            before: None,
            synced: true
        }
    );
    assert_eq!(fs::read(&path).unwrap(), b"new");
    assert_eq!(mode(&path), 0o755);
    assert_eq!(
        *ran.borrow(),
        [folder.path().join(".lamp-update-back.new")],
        "the probe runs the copy beside, never the fixed path"
    );
    assert_eq!(names(folder.path()), ["lamp-update-back"]);
}

#[test]
fn the_same_bytes_already_there_are_left_and_nothing_is_run() {
    let folder = folder();
    let path = folder.path().join("lamp-update-back");
    write(&path, b"same", 0o755);
    let ran = RefCell::new(Vec::new());
    let placed = place(&path, b"same", None, &mut good(&ran)).unwrap();
    assert_eq!(placed, Placed::Already);
    assert!(ran.borrow().is_empty());
    assert_eq!(names(folder.path()), ["lamp-update-back"]);
}

#[test]
fn the_same_bytes_not_executable_are_written_again() {
    let folder = folder();
    let path = folder.path().join("lamp-update-back");
    write(&path, b"same", 0o644);
    let ran = RefCell::new(Vec::new());
    let placed = place(&path, b"same", None, &mut good(&ran)).unwrap();
    assert_eq!(
        placed,
        Placed::Replaced {
            before: Some(sha256_hex(b"same")),
            synced: true,
        }
    );
    assert_eq!(mode(&path), 0o755);
}

#[test]
fn the_copy_replaced_is_kept_with_its_mode_before_the_rename() {
    let folder = folder();
    let path = folder.path().join("lamp-update-back");
    let kept = folder.path().join("lamp-update-back.prev");
    write(&path, b"old", 0o750);
    let ran = RefCell::new(Vec::new());
    let placed = place(&path, b"new", Some(&kept), &mut good(&ran)).unwrap();
    assert_eq!(
        placed,
        Placed::Replaced {
            before: Some(sha256_hex(b"old")),
            synced: true,
        }
    );
    assert_eq!(fs::read(&path).unwrap(), b"new");
    assert_eq!(fs::read(&kept).unwrap(), b"old");
    assert_eq!(mode(&kept), 0o750);
    assert_eq!(
        names(folder.path()),
        ["lamp-update-back", "lamp-update-back.prev"]
    );
}

#[test]
fn a_kept_copy_equal_to_the_one_there_is_used_and_another_one_refuses() {
    let folder = folder();
    let path = folder.path().join("lamp-update-back");
    let kept = folder.path().join("lamp-update-back.prev");
    write(&path, b"old", 0o755);
    write(&kept, b"old", 0o755);
    let ran = RefCell::new(Vec::new());
    place(&path, b"new", Some(&kept), &mut good(&ran)).unwrap();
    assert_eq!(fs::read(&kept).unwrap(), b"old");

    // Now the fixed path holds "new" and the kept copy "old": a third copy
    // would lose the only "old" there is.
    let ran = RefCell::new(Vec::new());
    let why = place(&path, b"third", Some(&kept), &mut good(&ran)).unwrap_err();
    assert!(why.contains("holds another copy"), "{why}");
    assert!(ran.borrow().is_empty(), "nothing is written or run");
    assert_eq!(fs::read(&path).unwrap(), b"new");
    assert_eq!(fs::read(&kept).unwrap(), b"old");
    assert_eq!(
        names(folder.path()),
        ["lamp-update-back", "lamp-update-back.prev"]
    );
}

#[test]
fn a_wrong_answer_or_none_leaves_everything_as_it_was() {
    type Probe = fn(&Path) -> Result<String, String>;
    let wrong: Probe = |_| Ok(answer(&sha256_hex(b"something else")));
    let old_protocol: Probe = |path| {
        Ok(format!(
            "lamp-update {PROBE} 0 {}",
            sha256_hex(&fs::read(path).unwrap())
        ))
    };
    let silent: Probe = |_| Ok(String::new());
    let failed: Probe = |_| Err("exit status: 101".to_owned());
    for (probe, said) in [
        (wrong, "answered"),
        (old_protocol, "answered"),
        (silent, "answered"),
        (failed, "did not answer"),
    ] {
        let folder = folder();
        let path = folder.path().join("lamp-update-back");
        let kept = folder.path().join("lamp-update-back.prev");
        write(&path, b"old", 0o755);
        let why = place(&path, b"new", Some(&kept), &mut { probe }).unwrap_err();
        assert!(why.contains(said) && why.contains("nothing"), "{why}");
        assert_eq!(fs::read(&path).unwrap(), b"old");
        assert_eq!(
            names(folder.path()),
            ["lamp-update-back"],
            "the new copy is removed and none is kept"
        );
    }
}

#[test]
fn a_link_or_folder_at_either_path_refuses_before_anything_is_written() {
    let folder = folder();
    let target = folder.path().join("target");
    write(&target, b"old", 0o755);
    let path = folder.path().join("lamp-update-back");
    symlink(&target, &path).unwrap();
    let ran = RefCell::new(Vec::new());
    let why = place(&path, b"new", None, &mut good(&ran)).unwrap_err();
    assert!(why.contains("not a regular file"), "{why}");

    let path = folder.path().join("fixed");
    write(&path, b"old", 0o755);
    let kept = folder.path().join("fixed.prev");
    fs::create_dir(&kept).unwrap();
    let why = place(&path, b"new", Some(&kept), &mut good(&ran)).unwrap_err();
    assert!(why.contains("not a regular file"), "{why}");
    assert!(ran.borrow().is_empty());
    assert_eq!(fs::read(&target).unwrap(), b"old");
    assert_eq!(fs::read(&path).unwrap(), b"old");
}

#[test]
fn a_copy_over_the_bound_is_refused_unwritten() {
    let folder = folder();
    let path = folder.path().join("lamp-update-back");
    let bytes = vec![0; MOST_BYTES as usize + 1];
    let ran = RefCell::new(Vec::new());
    let why = place(&path, &bytes, None, &mut good(&ran)).unwrap_err();
    assert!(why.contains("more than"), "{why}");
    assert!(names(folder.path()).is_empty());
}

/// A program that answers the probe as lamp-update does: the SHA-256 of
/// its own file.
const ANSWERS: &str = "#!/bin/sh\n\
    [ \"$1\" = writer-probe ] || exit 2\n\
    h=$( (sha256sum \"$0\" 2>/dev/null || shasum -a 256 \"$0\") | cut -d' ' -f1)\n\
    echo \"lamp-update writer-probe 1 $h\"\n";

#[test]
fn a_real_program_is_run_and_its_answer_checked() {
    let folder = folder();
    let path = folder.path().join("lamp-update-back");
    write(&path, b"old", 0o755);
    let placed = place(&path, ANSWERS.as_bytes(), None, &mut run_probe).unwrap();
    assert_eq!(
        placed,
        Placed::Replaced {
            before: Some(sha256_hex(b"old")),
            synced: true,
        }
    );
    assert_eq!(fs::read(&path).unwrap(), ANSWERS.as_bytes());

    // One that answers with another build's SHA-256, or fails, is not used.
    for script in [
        "#!/bin/sh\necho lamp-update writer-probe 1 0000\n",
        "#!/bin/sh\necho broken >&2\nexit 3\n",
    ] {
        let why = place(&path, script.as_bytes(), None, &mut run_probe).unwrap_err();
        assert!(why.contains("nothing was replaced"), "{why}");
        assert_eq!(fs::read(&path).unwrap(), ANSWERS.as_bytes());
        assert_eq!(names(folder.path()), ["lamp-update-back"]);
    }
}

#[test]
fn own_answer_names_the_running_file() {
    let exe = std::env::current_exe().unwrap();
    assert_eq!(
        own_answer().unwrap(),
        answer(&sha256_hex(&fs::read(exe).unwrap()))
    );
    assert_eq!(
        answer("ab"),
        "lamp-update writer-probe 1 ab",
        "the answer's form is a contract with older copies"
    );
}

#[test]
fn a_kept_copy_that_cannot_be_written_leaves_the_old_one_and_no_new_copy() {
    let folder = folder();
    let path = folder.path().join("lamp-update-back");
    let kept = folder.path().join("missing/lamp-update-back.prev");
    write(&path, b"old", 0o755);
    let ran = RefCell::new(Vec::new());
    let why = place(&path, b"new", Some(&kept), &mut good(&ran)).unwrap_err();
    assert!(why.contains("nothing was replaced"), "{why}");
    assert_eq!(ran.borrow().len(), 1, "the probe ran before the kept copy");
    assert_eq!(fs::read(&path).unwrap(), b"old");
    assert_eq!(names(folder.path()), ["lamp-update-back"]);
}
