use super::*;
use std::{fs, os::unix::fs::PermissionsExt};

const FOLDER: &str = "/etc/lampOS/expression/";

fn setting(name: &str, text: &str) -> Setting {
    Setting::new(&format!("{FOLDER}{name}"), text)
}

fn removed(name: &str) -> Setting {
    Setting {
        path: format!("{FOLDER}{name}"),
        text: None,
        sha256: None,
        mode: None,
        removed: true,
    }
}

fn put(root: &Path, setting: &Setting, bytes: &[u8]) {
    let path = root.join(setting.path.strip_prefix('/').unwrap());
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o640)).unwrap();
}

fn root() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap()
}

#[test]
fn captured_bytes_preserve_ordinary_ownership_and_keep_all_omitted_noops() {
    let root = root();
    let settings = vec![
        setting("owned", "new"),
        setting("custom", "new"),
        setting("record", "new"),
        setting("noop", "new"),
        setting("deleted", "new"),
        setting("writing-missing", "new"),
        removed("remove"),
        removed("keep-custom"),
        removed("absent"),
        setting("empty", "new"),
        setting("writing-present", "new"),
        setting("missing-parent/setting", "new"),
        setting("unrecorded-empty", "new"),
    ];
    for (at, bytes) in [
        (0, b"old".as_slice()),
        (1, b"custom"),
        (2, b"new"),
        (3, b"new"),
        (6, b"old"),
        (7, b"custom"),
        (9, b""),
        (10, b"old"),
        (12, b""),
    ] {
        put(root.path(), &settings[at], bytes);
    }
    let recorded = BTreeMap::from([
        (settings[0].path.clone(), manifest::sha256_hex(b"old")),
        (settings[3].path.clone(), manifest::sha256_hex(b"new")),
        (settings[4].path.clone(), install::SET_ASIDE.into()),
        (settings[6].path.clone(), manifest::sha256_hex(b"old")),
        (settings[9].path.clone(), manifest::sha256_hex(b"")),
    ]);
    let writing = BTreeMap::from([
        (settings[5].path.clone(), manifest::sha256_hex(b"old")),
        (settings[10].path.clone(), manifest::sha256_hex(b"old")),
    ]);
    let captured = capture(root.path(), &settings, &recorded, &writing).unwrap();
    captured.revalidate(root.path()).unwrap();
    captured.revalidate(root.path()).unwrap();
    assert_eq!(captured.entries().len(), settings.len());
    let actions: Vec<_> = captured.entries().iter().map(Captured::act).collect();
    assert_eq!(actions[0], Some(&Act::Write));
    assert!(matches!(actions[1], Some(Act::Keep(_))));
    assert_eq!(actions[2], Some(&Act::Record));
    assert_eq!(actions[3], None);
    assert!(matches!(actions[4], Some(Act::Keep(_))));
    assert_eq!(actions[5], Some(&Act::Write));
    assert_eq!(actions[6], Some(&Act::Remove));
    assert!(matches!(actions[7], Some(Act::Keep(_))));
    assert_eq!(actions[8], None);
    assert_eq!(actions[9], Some(&Act::Write));
    assert_eq!(actions[10], Some(&Act::Write));
    assert_eq!(actions[11], Some(&Act::Write));
    assert!(matches!(actions[12], Some(Act::Keep(_))));
    let expected = apply::plan(
        root.path(),
        &settings,
        &recorded,
        &writing,
        &BTreeMap::new(),
    )
    .unwrap();
    let actual: Vec<_> = captured
        .entries()
        .iter()
        .filter_map(|entry| {
            Some(apply::Step {
                setting: entry.setting().clone(),
                act: entry.act()?.clone(),
            })
        })
        .collect();
    assert_eq!(actual, expected);
    assert!(matches!(
        captured.entries()[4].before().entry,
        Entry::MissingLeaf
    ));
    assert!(matches!(
        captured.entries()[11].before().entry,
        Entry::MissingAncestor { .. }
    ));
    let Entry::Present { bytes, identity } = &captured.entries()[3].before().entry else {
        panic!("the exact existing value was not captured");
    };
    assert_eq!(bytes, b"new");
    assert_eq!(identity.mode & 0o7777, 0o640);
    assert_eq!(captured.entries()[4].recorded(), Some(install::SET_ASIDE));
    assert_eq!(
        captured.entries()[10].writing(),
        writing.get(&settings[10].path).map(String::as_str)
    );
}

#[test]
fn old_byte_limits_allow_exact_budget_then_empty_or_absent_but_never_a_prefix() {
    let root = root();
    let mut settings: Vec<_> = (0..4)
        .map(|n| setting(&format!("large-{n}"), "new"))
        .collect();
    let bytes = vec![42; writer_files::MOST_BYTES];
    for input in &settings {
        put(root.path(), input, &bytes);
    }
    settings.push(setting("empty-after-budget", "new"));
    settings.push(setting("absent-after-budget", "new"));
    put(root.path(), &settings[4], b"");
    let recorded = BTreeMap::new();
    let writing = BTreeMap::new();
    let captured = capture(root.path(), &settings, &recorded, &writing).unwrap();
    assert_eq!(captured.old_bytes(), MOST_OLD_BYTES);
    captured.revalidate(root.path()).unwrap();
    assert!(matches!(
        &captured.entries()[4].before().entry,
        Entry::Present { bytes, .. } if bytes.is_empty()
    ));
    assert!(matches!(
        captured.entries()[5].before().entry,
        Entry::MissingLeaf
    ));
    drop(captured);
    put(root.path(), &settings[4], b"x");
    assert!(capture(root.path(), &settings, &recorded, &writing).is_err());
    put(
        root.path(),
        &settings[0],
        &vec![42; writer_files::MOST_BYTES + 1],
    );
    assert!(capture(root.path(), &settings[..1], &recorded, &writing).is_err());
}

#[test]
fn selected_invalid_values_refuse_before_filesystem_access_and_unrelated_values_are_ignored() {
    let root = root();
    let settings = vec![setting("setting", "new")];
    let absent = root.path().join("not-a-root");
    let empty = BTreeMap::new();
    for value in ["invalid private record".into(), "x".repeat(1024 * 1024)] {
        let selected = BTreeMap::from([(settings[0].path.clone(), value)]);
        let error = capture(&absent, &settings, &selected, &empty).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(!format!("{error:?}").contains("private record"));
    }
    let writing_marker = BTreeMap::from([(settings[0].path.clone(), install::SET_ASIDE.into())]);
    assert_eq!(
        capture(&absent, &settings, &empty, &writing_marker)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData
    );
    let unrelated = BTreeMap::from([("unrelated private key".into(), "x".repeat(5 * 1024 * 1024))]);
    let captured = capture(root.path(), &settings, &unrelated, &empty).unwrap();
    assert_eq!(captured.entries()[0].act(), Some(&Act::Write));
    assert_eq!(unrelated.values().next().unwrap().len(), 5 * 1024 * 1024);
}

#[test]
fn oversized_or_invalid_new_inputs_are_bounded_and_redacted_before_observation() {
    let root = root();
    let absent = root.path().join("not-a-root");
    let empty = BTreeMap::new();
    let valid = setting("private-filename", "private-contents");
    let mut cases = Vec::new();
    let mut value = valid.clone();
    value.text = Some("private-text".repeat(manifest::MOST_SETTINGS_BYTES));
    cases.push(vec![value]);
    let mut value = valid.clone();
    value.path = "private-path".repeat(1024);
    cases.push(vec![value]);
    let mut value = valid.clone();
    value.sha256 = Some("private-digest".repeat(1024));
    cases.push(vec![value]);
    let mut value = valid.clone();
    value.path = format!("{FOLDER}{}", vec!["x"; 65].join("/"));
    cases.push(vec![value]);
    cases.push(vec![valid.clone(), valid.clone()]);
    cases.push(vec![valid.clone(); manifest::MOST_SETTINGS + 1]);
    for inputs in cases {
        let error = capture(&absent, &inputs, &empty, &empty).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(!format!("{error:?}").contains("private-"));
    }
    put(root.path(), &valid, b"private-old-contents");
    let inputs = [valid];
    let captured = capture(root.path(), &inputs, &empty, &empty).unwrap();
    let debug = format!("{captured:?} {:?}", captured.entries()[0]);
    assert!(!debug.contains("private-"));
}

#[test]
fn shared_classifier_keeps_tmpfiles_marker_and_image_shipped_precedence() {
    let setting = Setting::new(install::TMPFILES_PATH, "new");
    let old = apply::TMPFILES_SHIPPED[0];
    let empty = BTreeMap::new();
    let no_shipped = BTreeMap::new();
    assert_eq!(
        apply::observed_act(&setting, Some(old), &empty, &empty, &no_shipped),
        Some(Act::Write)
    );
    let recorded = BTreeMap::from([(setting.path.clone(), "f".repeat(64))]);
    assert!(matches!(
        apply::observed_act(&setting, Some(old), &recorded, &empty, &no_shipped),
        Some(Act::Keep(_))
    ));
    let writing = BTreeMap::from([(setting.path.clone(), old.into())]);
    assert_eq!(
        apply::observed_act(&setting, Some(old), &recorded, &writing, &no_shipped),
        Some(Act::Write)
    );
    let shipped = BTreeMap::from([(setting.path.clone(), vec![old.into()])]);
    assert_eq!(
        apply::observed_act(&setting, Some(old), &recorded, &empty, &shipped),
        Some(Act::Write)
    );
    let desired = setting.sha256.as_deref();
    assert_eq!(
        apply::observed_act(&setting, desired, &recorded, &empty, &shipped),
        Some(Act::Record)
    );
}

#[test]
fn revalidation_refuses_byte_mode_deletion_and_same_byte_inode_changes() {
    for change in 0..6 {
        let root = root();
        let settings = [setting("private-setting", "new")];
        let recorded = BTreeMap::from([(
            settings[0].path.clone(),
            manifest::sha256_hex(b"private-old"),
        )]);
        let writing = BTreeMap::new();
        put(root.path(), &settings[0], b"private-old");
        let captured = capture(root.path(), &settings, &recorded, &writing).unwrap();
        captured.revalidate(root.path()).unwrap();
        let path = root
            .path()
            .join(settings[0].path.strip_prefix('/').unwrap());
        match change {
            0 => fs::write(&path, b"PRIVATE-OLD").unwrap(),
            1 => fs::write(&path, b"short").unwrap(),
            2 => fs::write(&path, b"private-old!").unwrap(),
            3 => fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap(),
            4 => {
                let replacement = root.path().join("replacement");
                fs::write(&replacement, b"private-old").unwrap();
                fs::set_permissions(&replacement, fs::Permissions::from_mode(0o640)).unwrap();
                fs::rename(replacement, &path).unwrap();
            }
            5 => fs::remove_file(&path).unwrap(),
            _ => unreachable!(),
        }
        let error = captured.revalidate(root.path()).unwrap_err();
        if change == 2 {
            // One byte of growth, far below the observer's global bound,
            // must be refused by the smaller original-size read cap.
            assert_eq!(error.to_string(), "observation file exceeds its byte cap");
        }
        assert!(!format!("{error:?}").contains("private-"));
        let Entry::Present { bytes, .. } = &captured.entries()[0].before().entry else {
            panic!("revalidation lost the original preimage");
        };
        assert_eq!(bytes, b"private-old");
        assert_eq!(captured.entries()[0].act(), Some(&Act::Write));
    }
}

#[test]
fn revalidation_refuses_new_negative_dependencies_and_replaced_ancestors() {
    for change in 0..3 {
        let root = root();
        let name = if change == 0 {
            "missing"
        } else {
            "nested/setting"
        };
        let settings = [setting(name, "new")];
        let empty = BTreeMap::new();
        fs::create_dir_all(root.path().join(FOLDER.trim_start_matches('/'))).unwrap();
        if change == 2 {
            put(root.path(), &settings[0], b"old");
        }
        let captured = capture(root.path(), &settings, &empty, &empty).unwrap();
        captured.revalidate(root.path()).unwrap();
        if change == 0 {
            // A zero-byte file is still a change from an explicit absence.
            put(root.path(), &settings[0], b"");
        } else if change == 1 {
            // The final leaf remains absent, but a missing ancestor appeared.
            fs::create_dir(root.path().join("etc/lampOS/expression/nested")).unwrap();
        } else {
            let folder = root.path().join("etc/lampOS/expression/nested");
            fs::rename(&folder, root.path().join("saved-ancestor")).unwrap();
            put(root.path(), &settings[0], b"old");
            let now = writer_files::observe(root.path(), &settings[0].path[1..], 3).unwrap();
            assert_ne!(
                captured.entries()[0].before().ancestors.last(),
                now.ancestors.last()
            );
        }
        assert!(captured.revalidate(root.path()).is_err());
    }
}

#[test]
fn revalidation_refuses_replaced_roots_links_and_nonregular_leaves() {
    for change in 0..3 {
        let container = root();
        let root = container.path().join("root");
        fs::create_dir(&root).unwrap();
        let settings = [setting("private-setting", "new")];
        let empty = BTreeMap::new();
        put(&root, &settings[0], b"private-old");
        let captured = capture(&root, &settings, &empty, &empty).unwrap();
        let path = root.join(settings[0].path.strip_prefix('/').unwrap());
        if change == 0 {
            fs::rename(&root, container.path().join("previous-root")).unwrap();
            fs::create_dir(&root).unwrap();
            put(&root, &settings[0], b"private-old");
        } else {
            fs::remove_file(&path).unwrap();
            if change == 1 {
                let outside = container.path().join("private-outside");
                fs::write(&outside, b"private-old").unwrap();
                std::os::unix::fs::symlink(outside, &path).unwrap();
            } else {
                fs::create_dir(&path).unwrap();
            }
        }
        let error = captured.revalidate(&root).unwrap_err();
        assert!(!format!("{error:?}").contains("private-"));
    }
    let container = root();
    let empty = BTreeMap::new();
    let captured = capture(container.path(), &[], &empty, &empty).unwrap();
    // No input means no root was observed; success grants no root authority.
    captured
        .revalidate(&container.path().join("absent"))
        .unwrap();
}
