use super::*;
use std::os::unix::fs::symlink;

const BUILD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OTHER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

struct Lamp {
    root: tempfile::TempDir,
    records: PathBuf,
}

/// A lamp whose `update/current` names BUILD, holding `bytes`, and OTHER
/// beside it, holding "old".
fn lamp(bytes: &[u8]) -> Lamp {
    let root = tempfile::tempdir().unwrap();
    let update = on_root(root.path(), FOLDER);
    for (build, bytes) in [(BUILD, bytes), (OTHER, b"old".as_slice())] {
        fs::create_dir_all(update.join(build)).unwrap();
        fs::write(update.join(build).join(BINARY), bytes).unwrap();
    }
    symlink(BUILD, on_root(root.path(), CURRENT)).unwrap();
    let records = root.path().join("var/lib/lampos-update");
    fs::create_dir_all(&records).unwrap();
    Lamp { root, records }
}

fn relink(lamp: &Lamp, target: &str) {
    let link = on_root(lamp.root.path(), CURRENT);
    fs::remove_file(&link).unwrap();
    symlink(target, link).unwrap();
}

#[test]
fn the_confirmed_build_running_with_no_switch_waiting_is_ready() {
    let lamp = lamp(b"new");
    ready(lamp.root.path(), &lamp.records, b"new").unwrap();
}

#[test]
fn a_switch_waiting_or_one_that_cannot_be_read_is_not_settled() {
    let lamp = lamp(b"new");
    own::begin(&lamp.records, OTHER, BUILD).unwrap();
    let why = ready(lamp.root.path(), &lamp.records, b"new").unwrap_err();
    assert!(why.contains("waits to be confirmed"), "{why}");

    fs::write(lamp.records.join("self.json"), b"{not json").unwrap();
    let why = ready(lamp.root.path(), &lamp.records, b"new").unwrap_err();
    assert!(why.contains("not settled"), "{why}");
}

#[test]
fn a_running_lamp_update_that_is_not_the_build_named_is_not_ready() {
    let lamp = lamp(b"new");
    // Put back to OTHER while this one still runs.
    relink(&lamp, OTHER);
    let why = ready(lamp.root.path(), &lamp.records, b"new").unwrap_err();
    assert!(why.contains("not the build"), "{why}");
    // The build named, changed under the same name since this one started.
    let lamp = super::tests::lamp(b"changed");
    let why = ready(lamp.root.path(), &lamp.records, b"new").unwrap_err();
    assert!(why.contains("not the build"), "{why}");
}

#[test]
fn a_link_that_names_no_build_or_a_binary_that_is_no_file_is_refused() {
    let lamp = lamp(b"new");
    for target in [
        "/opt/lampOS/update/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "../update/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "aaaa",
        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
    ] {
        relink(&lamp, target);
        assert!(
            ready(lamp.root.path(), &lamp.records, b"new").is_err(),
            "{target}"
        );
    }

    let lamp = super::tests::lamp(b"new");
    let binary = on_root(lamp.root.path(), FOLDER).join(BUILD).join(BINARY);
    fs::remove_file(&binary).unwrap();
    symlink(
        on_root(lamp.root.path(), FOLDER).join(OTHER).join(BINARY),
        &binary,
    )
    .unwrap();
    let why = ready(lamp.root.path(), &lamp.records, b"old").unwrap_err();
    assert!(why.contains("not a regular file"), "{why}");

    fs::remove_file(on_root(lamp.root.path(), CURRENT)).unwrap();
    assert!(ready(lamp.root.path(), &lamp.records, b"new").is_err());
}
