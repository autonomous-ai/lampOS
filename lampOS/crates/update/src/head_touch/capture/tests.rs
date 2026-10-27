use super::*;
use crate::{
    apply::{Act, Step},
    head_touch,
    manifest::Setting,
};
use std::{
    fs,
    os::unix::{
        fs::{PermissionsExt, symlink},
        net::UnixListener,
    },
};

fn root() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap()
}

fn put(root: &Path, relative: &str, text: &[u8]) -> PathBuf {
    let path = root.join(relative.trim_start_matches('/'));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, text).unwrap();
    path
}

fn make_fifo(path: &Path) {
    // Rustix has no mkfifoat on Apple. Reuse install's portable fixture
    // utility pattern, with an owned/reaped direct child even on panic.
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = Child(
        std::process::Command::new("mkfifo")
            .args(["-m", "600"])
            .arg(path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success(), "private FIFO fixture failed");
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "private FIFO fixture setup deadline"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn fixture(head: bool) -> (tempfile::TempDir, BTreeMap<String, String>, Setting) {
    let root = root();
    let unit = |name: &str, args: &str, environment: &str| {
        format!("[Service]\nExecStart=/opt/lampOS/{name}/current/lamp-{name} {args}\n{environment}")
    };
    let units = BTreeMap::from([
        (
            "lamp-controls.service".into(),
            unit("controls", &format!("--pins {}", head_touch::PINS), ""),
        ),
        (
            "lamp-voice.service".into(),
            unit(
                "voice",
                "--controls-mute /run/lampos-bus/mute",
                &format!(
                    "Environment=LAMP_HEAD_TOUCH={}\n",
                    if head { "on" } else { "off" }
                ),
            ),
        ),
        (
            "lamp-expression.service".into(),
            unit("expression", if head { "--head-touch" } else { "" }, ""),
        ),
        (
            "lamp-body.service".into(),
            unit(
                "body",
                "%H $BODY_POSES",
                "Environment=LAMP_HEAD_TOUCH=on\nEnvironmentFile=/etc/lampOS/body/lamp.env\nEnvironmentFile=-/var/lib/lampos-body/measured.env\n",
            ),
        ),
    ]);
    for (name, text) in &units {
        put(
            root.path(),
            &format!("etc/systemd/system/{name}"),
            text.as_bytes(),
        );
    }
    put(
        root.path(),
        "etc/lampOS/body/lamp.env",
        b"BODY_POSES=\"1,2,3\"\n",
    );
    fs::create_dir(root.path().join("etc/systemd/system/service.d")).unwrap();
    let setting = Setting::new(
        head_touch::PINS,
        r#"{"head_pads":[{"name":"head-1","line":41,"active_low":true,"pull":"up"}],"legacy_head":false}"#,
    );
    put(
        root.path(),
        head_touch::PINS,
        setting.text.as_deref().unwrap().as_bytes(),
    );
    (root, units, setting)
}

#[test]
fn actual_projection_and_verification_replay_after_the_filesystem_is_gone() {
    for head in [false, true] {
        for omitted in [false, true] {
            for carried in [false, true] {
                let (root, all, setting) = fixture(head);
                let root_path = root.path().to_owned();
                let units = root_path.join("etc/systemd/system");
                let carried = if carried { all } else { BTreeMap::new() };
                let settings = [setting];
                let initial = if omitted {
                    Vec::new()
                } else {
                    vec![Step {
                        setting: settings[0].clone(),
                        act: Act::Write,
                    }]
                };
                let mut expected = initial.clone();
                head_touch::project(&root_path, &units, &carried, &settings, &mut expected)
                    .unwrap();
                head_touch::verify(&root_path, &units, &carried, &expected).unwrap();
                let (observed, mut frozen) = collect(&root_path, |inputs| {
                    let mut steps = initial.clone();
                    head_touch::project_with(
                        inputs, &root_path, &units, &carried, &settings, &mut steps,
                    )?;
                    head_touch::verify_with(inputs, &root_path, &units, &carried, &steps)?;
                    Ok(steps)
                })
                .unwrap();
                assert_eq!(observed, expected);
                assert_eq!(frozen.directories.len(), if head { 30 } else { 25 });
                assert_eq!(
                    frozen
                        .texts
                        .contains_key(&root_path.join("etc/lampOS/body/lamp.env")),
                    head
                );
                frozen.revalidate(&root_path).unwrap();
                root.close().unwrap();
                let mut replayed = initial;
                head_touch::project_with(
                    &mut frozen,
                    &root_path,
                    &units,
                    &carried,
                    &settings,
                    &mut replayed,
                )
                .unwrap();
                head_touch::verify_with(&mut frozen, &root_path, &units, &carried, &replayed)
                    .unwrap();
                assert_eq!(replayed, expected);
                assert!(frozen.revalidate(&root_path).is_err());
            }
        }
    }
}

#[test]
fn custom_pins_and_settings_off_do_not_invent_product_or_body_dependencies() {
    let (root, mut carried, _) = fixture(false);
    let custom = "/etc/lampOS/custom-pins.json";
    let controls = carried.get_mut("lamp-controls.service").unwrap();
    *controls = controls.replace(head_touch::PINS, custom);
    put(
        root.path(),
        custom,
        br#"{"head_pads":[1],"legacy_head":true}"#,
    );
    let units = root.path().join("etc/systemd/system");
    let (_, mut frozen) = collect(root.path(), |inputs| {
        let mut steps = Vec::new();
        head_touch::project_with(inputs, root.path(), &units, &carried, &[], &mut steps)?;
        head_touch::verify_with(inputs, root.path(), &units, &carried, &steps)
    })
    .unwrap();
    assert!(
        frozen
            .text(&root.path().join("etc/lampOS/custom-pins.json"))
            .unwrap()
            .is_some()
    );
    assert!(
        frozen
            .text(&root.path().join(head_touch::PINS.trim_start_matches('/')))
            .is_err()
    );
    assert!(
        frozen
            .text(&root.path().join("etc/lampOS/body/lamp.env"))
            .is_err()
    );
}

#[test]
fn absence_empty_regular_and_null_are_distinct_and_unknown_keys_are_errors() {
    let root = root();
    let empty = put(root.path(), "a/empty", b"");
    symlink("/dev/null", root.path().join("a/mask")).unwrap();
    let missing = root.path().join("a/missing");
    let absent_parent = root.path().join("a/absent/file");
    let (_, mut frozen) = collect(root.path(), |inputs| {
        assert_eq!(inputs.text(&empty)?, Some(String::new()));
        assert_eq!(
            inputs.text(&root.path().join("a/mask"))?,
            Some(String::new())
        );
        assert_eq!(inputs.text(&missing)?, None);
        assert_eq!(inputs.text(&absent_parent)?, None);
        assert!(inputs.entries(&root.path().join("a/absent"))?.is_none());
        Ok(())
    })
    .unwrap();
    assert!(matches!(frozen.texts[&empty].text, Text::Regular { .. }));
    assert!(matches!(
        frozen.texts[&root.path().join("a/mask")].text,
        Text::NullMask { .. }
    ));
    assert!(matches!(frozen.texts[&missing].text, Text::MissingLeaf));
    assert!(matches!(
        frozen.texts[&absent_parent].text,
        Text::MissingAncestor { .. }
    ));
    assert!(frozen.text(&root.path().join("never-asked")).is_err());
    assert!(frozen.entries(&root.path().join("never-asked")).is_err());
    assert!(!format!("{frozen:?}").contains(&root.path().display().to_string()));
}

#[test]
fn a_fixture_null_spelling_is_regular_text_and_a_real_null_mask_is_metadata_only() {
    let root = root();
    let fake_null = put(root.path(), "dev/null", b"not a device");
    symlink(&fake_null, root.path().join("named-null")).unwrap();
    symlink("/dev/null", root.path().join("real-mask")).unwrap();
    let (_, frozen) = collect(root.path(), |inputs| {
        assert_eq!(
            inputs.text(&root.path().join("named-null"))?.as_deref(),
            Some("not a device")
        );
        assert_eq!(
            inputs.text(&root.path().join("real-mask"))?.as_deref(),
            Some("")
        );
        Ok(())
    })
    .unwrap();
    assert!(matches!(
        frozen.texts[&root.path().join("named-null")].text,
        Text::Regular { .. }
    ));
    assert!(matches!(
        frozen.texts[&root.path().join("real-mask")].text,
        Text::NullMask { .. }
    ));
}

#[test]
fn relative_directory_and_regular_links_and_contained_absolute_links_recheck() {
    let root = root();
    let target = put(root.path(), "a/real/value", b"literal");
    symlink("real", root.path().join("a/alias")).unwrap();
    symlink("real/value", root.path().join("a/link")).unwrap();
    symlink(&target, root.path().join("a/absolute")).unwrap();
    let (_, mut frozen) = collect(root.path(), |inputs| {
        for path in ["a/alias/value", "a/link", "a/absolute"] {
            assert_eq!(
                inputs.text(&root.path().join(path))?.as_deref(),
                Some("literal")
            );
        }
        let entries = inputs
            .entries(&root.path().join("a/alias"))?
            .unwrap()
            .collect::<Result<Vec<_>>>()?;
        assert_eq!(entries, [root.path().join("a/alias/value")]);
        Ok(())
    })
    .unwrap();
    assert!(frozen.text(&target).is_err());
    frozen.revalidate(root.path()).unwrap();
    fs::remove_file(root.path().join("a/link")).unwrap();
    symlink("real/other", root.path().join("a/link")).unwrap();
    assert!(frozen.revalidate(root.path()).is_err());
}

#[test]
fn cache_keys_and_link_targets_keep_directory_requiring_spellings_distinct() {
    let root = root();
    let path = put(root.path(), "a/value", b"literal");
    let (_, mut frozen) = collect(root.path(), |inputs| inputs.text(&path)).unwrap();
    for spelling in ["a/value/", "a/value/.", "a//value"] {
        let request = root.path().join(spelling);
        assert!(frozen.text(&request).is_err());
        assert!(collect(root.path(), |inputs| inputs.text(&request)).is_err());
    }
    for (name, target) in [("slash", "value/"), ("dot", "value/.")] {
        symlink(target, root.path().join("a").join(name)).unwrap();
        assert!(
            collect(root.path(), |inputs| inputs
                .text(&root.path().join("a").join(name)))
            .is_err()
        );
    }
    let absolute = OsString::from_vec([path.as_os_str().as_bytes(), b"/."].concat());
    symlink(absolute, root.path().join("a/absolute-dot")).unwrap();
    assert!(
        collect(root.path(), |inputs| inputs
            .text(&root.path().join("a/absolute-dot")))
        .is_err()
    );
    symlink("/dev/null/", root.path().join("a/not-a-mask")).unwrap();
    assert!(
        collect(root.path(), |inputs| inputs
            .text(&root.path().join("a/not-a-mask")))
        .is_err()
    );
}

#[test]
fn escapes_and_link_loops_refuse_without_an_ordinary_reader_fallback() {
    let outside = root();
    let root = root();
    put(outside.path(), "secret", b"private-value");
    symlink(outside.path().join("secret"), root.path().join("escape")).unwrap();
    symlink("../outside", root.path().join("up")).unwrap();
    symlink("loop", root.path().join("loop")).unwrap();
    for path in ["escape", "up", "loop"] {
        let mut collector = Collector::open(root.path(), Limits::default()).unwrap();
        assert_eq!(
            collector.text(&root.path().join(path)).unwrap_err(),
            REFUSED
        );
        assert!(collector.finish().is_err());
    }
}

#[test]
fn dangling_link_appearance_and_missing_ancestor_creation_invalidate() {
    for ancestor in [false, true] {
        let root = root();
        put(root.path(), "a/existing", b"");
        let relative = if ancestor { "new/file" } else { "new" };
        symlink(relative, root.path().join("a/link")).unwrap();
        let (_, frozen) = collect(root.path(), |inputs| {
            assert_eq!(inputs.text(&root.path().join("a/link"))?, None);
            Ok(())
        })
        .unwrap();
        put(root.path(), &format!("a/{relative}"), b"appeared");
        assert!(frozen.revalidate(root.path()).is_err());
    }
}

#[test]
fn directory_membership_including_ignored_names_and_absence_is_rechecked() {
    for present in [false, true] {
        let root = root();
        put(root.path(), "a/keep", b"");
        let directory = root.path().join("a/drop");
        if present {
            fs::create_dir(&directory).unwrap();
        }
        let (_, frozen) = collect(root.path(), |inputs| {
            assert_eq!(inputs.entries(&directory)?.is_some(), present);
            Ok(())
        })
        .unwrap();
        put(root.path(), "a/drop/ignored.txt", b"");
        assert!(frozen.revalidate(root.path()).is_err());
    }
}

#[test]
fn removal_from_a_captured_directory_invalidates_without_a_text_dependency() {
    let root = root();
    let entry = put(root.path(), "a/drop/ignored", b"");
    let (_, frozen) = collect(root.path(), |inputs| {
        let names = inputs
            .entries(&root.path().join("a/drop"))?
            .unwrap()
            .collect::<Result<Vec<_>>>()?;
        assert_eq!(names, vec![entry.clone()]);
        Ok(())
    })
    .unwrap();
    assert!(frozen.texts.is_empty());
    fs::remove_file(entry).unwrap();
    assert!(frozen.revalidate(root.path()).is_err());
}

#[test]
fn file_mutation_and_ancestor_replacement_refuse_before_freeze() {
    for replace_parent in [false, true] {
        let root = root();
        let path = put(root.path(), "a/nested/value", b"first");
        let mut collector = Collector::open(root.path(), Limits::default()).unwrap();
        assert_eq!(collector.text(&path).unwrap().as_deref(), Some("first"));
        if replace_parent {
            fs::rename(root.path().join("a/nested"), root.path().join("a/old")).unwrap();
            put(root.path(), "a/nested/value", b"first");
        } else {
            // Equal-length content changes must be caught by the final fresh
            // byte comparison even if the filesystem timestamps coincide.
            fs::write(&path, b"other").unwrap();
        }
        assert!(collector.finish().is_err());
    }

    // Model equal metadata deterministically: retain the old captured bytes,
    // but replace every metadata/link witness with the modified file's fresh
    // observation. This is a private fixture, not an admitted snapshot. The
    // final comparison must still reject the different complete contents.
    let root = root();
    let path = put(root.path(), "value", b"first");
    let mut collector = Collector::open(root.path(), Limits::default()).unwrap();
    assert_eq!(collector.text(&path).unwrap().as_deref(), Some("first"));
    fs::write(&path, b"other").unwrap();
    let mut same_metadata = collector.observe_text(&path).unwrap();
    let Text::Regular { text, .. } = &mut same_metadata.text else {
        panic!("regular file fixture required");
    };
    assert_eq!(text, "other");
    *text = "first".into();
    collector.texts.insert(path, same_metadata);
    assert!(collector.finish().is_err());
}

#[test]
fn same_descriptor_checks_reject_preopen_swap_and_postread_mutation() {
    for before in [true, false] {
        let root = root();
        let path = put(root.path(), "value", b"first");
        let directory = File::open(root.path()).unwrap();
        let expected =
            Identity(rustix::fs::statat(&directory, "value", AtFlags::SYMLINK_NOFOLLOW).unwrap());
        let outcome = read_regular(
            &directory,
            OsStr::new("value"),
            expected,
            100,
            || {
                if before {
                    fs::remove_file(&path).unwrap();
                    symlink("/dev/zero", &path).unwrap();
                }
            },
            || {
                if !before {
                    // This primitive promises metadata/length checks. A size
                    // change makes the post-read refusal deterministic without
                    // assuming that timestamps advance for equal-length writes.
                    fs::write(&path, b"changed-size").unwrap();
                }
            },
        );
        assert!(outcome.is_err());
    }
}

#[test]
fn metadata_only_null_classification_refuses_zero_fifo_and_socket() {
    let root = root();
    symlink("/dev/zero", root.path().join("zero")).unwrap();
    let socket = root.path().join("socket");
    let _listener = UnixListener::bind(&socket).unwrap();
    let fifo = root.path().join("fifo");
    make_fifo(&fifo);
    for path in [root.path().join("zero"), socket, fifo] {
        assert!(collect(root.path(), |inputs| inputs.text(&path)).is_err());
    }
    // Actual host null and zero metadata distinguish devices without a read.
    assert!(collect(Path::new("/"), |inputs| inputs.text(Path::new("/dev/zero"))).is_err());
}

#[test]
fn raw_non_utf8_names_are_preserved_but_non_utf8_text_is_refused() {
    let root = root();
    let directory = root.path().join("drop");
    let name = OsString::from_vec(vec![b'x', 0xff]);
    let path = directory.join(&name);
    let regular = put(root.path(), "drop/value", b"valid");
    let (_, mut frozen) = collect(root.path(), |inputs| {
        let paths = inputs
            .entries(&directory)?
            .unwrap()
            .collect::<Result<Vec<_>>>()?;
        assert_eq!(paths, vec![regular.clone()]);
        assert_eq!(inputs.text(&regular)?.as_deref(), Some("valid"));
        Ok(())
    })
    .unwrap();

    // Portable content refusal uses an ordinary filename, so the host's name
    // encoding cannot prevent reaching the collector. Even a caught provider
    // error must leave no usable frozen result.
    fs::write(&regular, [0xff]).unwrap();
    assert!(
        collect(root.path(), |inputs| {
            let _ = inputs.text(&regular);
            Ok(())
        })
        .is_err()
    );

    // A closed-replay encoding fixture, not an admitted physical snapshot:
    // substitute an opaque name/key in the private captured facts. Replay must
    // preserve its exact bytes without an OS lookup or lossy conversion. The
    // real bad-byte filesystem capture/recheck runs on Linux below.
    let fact = frozen.texts.remove(&regular).unwrap();
    assert!(frozen.texts.insert(path.clone(), fact).is_none());
    let members = frozen.directories.get_mut(&directory).unwrap();
    assert_eq!(
        members.names.as_deref(),
        Some([OsString::from("value")].as_slice())
    );
    members.names = Some(vec![name.clone()]);
    root.close().unwrap();
    assert_eq!(
        frozen
            .entries(&directory)
            .unwrap()
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .file_name()
            .unwrap()
            .as_bytes(),
        name.as_bytes()
    );
    assert_eq!(frozen.text(&path).unwrap().as_deref(), Some("valid"));
    assert!(frozen.text(&regular).is_err());

    // Linux is the product target and represents this physical filename. The
    // observed Mac filesystem returns EILSEQ before a collector is called.
    // Do not catch that error, ignore the whole test, or claim Mac FS coverage.
    #[cfg(target_os = "linux")]
    {
        let physical = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let directory = physical.path().join("drop");
        fs::create_dir(&directory).unwrap();
        let path = directory.join(&name);
        fs::write(&path, b"valid").unwrap();
        let (_, mut captured) = collect(physical.path(), |inputs| {
            let paths = inputs
                .entries(&directory)?
                .unwrap()
                .collect::<Result<Vec<_>>>()?;
            assert_eq!(paths, vec![path.clone()]);
            assert_eq!(paths[0].file_name().unwrap().as_bytes(), name.as_bytes());
            assert_eq!(inputs.text(&path)?.as_deref(), Some("valid"));
            Ok(())
        })
        .unwrap();
        captured.revalidate(physical.path()).unwrap();
        assert_eq!(captured.text(&path).unwrap().as_deref(), Some("valid"));
        fs::write(&path, [0xff]).unwrap();
        assert!(captured.revalidate(physical.path()).is_err());
        assert!(collect(physical.path(), |inputs| inputs.text(&path)).is_err());
    }
}

#[test]
fn regular_hard_links_retain_link_count_and_content_dependencies() {
    let root = root();
    let path = put(root.path(), "a/value", b"literal");
    fs::hard_link(&path, root.path().join("a/alias")).unwrap();
    let (_, frozen) = collect(root.path(), |inputs| inputs.text(&path)).unwrap();
    frozen.revalidate(root.path()).unwrap();
    fs::remove_file(root.path().join("a/alias")).unwrap();
    assert!(frozen.revalidate(root.path()).is_err());
}

#[test]
fn root_substitution_and_an_unrelated_root_refuse_revalidation() {
    let outer = root();
    let read_root = outer.path().join("root");
    fs::create_dir(&read_root).unwrap();
    let file = put(&read_root, "value", b"same");
    let (_, frozen) = collect(&read_root, |inputs| inputs.text(&file)).unwrap();
    assert!(frozen.revalidate(outer.path()).is_err());
    fs::rename(&read_root, outer.path().join("old")).unwrap();
    put(&read_root, "value", b"same");
    assert!(frozen.revalidate(&read_root).is_err());
}

#[test]
fn any_caught_provider_error_poisoned_collection_and_policy_errors_are_redacted() {
    let root = root();
    let file = put(root.path(), "private-calibration-name", b"private-value");
    assert!(
        collect(root.path(), |inputs| {
            let _ = inputs.entries(&file);
            Ok(())
        })
        .is_err()
    );
    let result: Result<((), FrozenInputs)> =
        collect(root.path(), |_| Err("secret caller path/value".into()));
    assert_eq!(result.unwrap_err(), REFUSED);
}

#[test]
fn file_aggregate_directory_name_key_and_work_limits_return_no_partial_success() {
    let root = root();
    let one = put(root.path(), "d/one", b"12");
    let two = put(root.path(), "d/two", b"34");
    let scenarios = [
        Limits {
            file: 1,
            ..Limits::default()
        },
        Limits {
            bytes: 3,
            ..Limits::default()
        },
        Limits {
            keys: 1,
            ..Limits::default()
        },
        Limits {
            work: 1,
            ..Limits::default()
        },
    ];
    for limits in scenarios {
        let mut collector = Collector::open(root.path(), limits).unwrap();
        let _ = collector.text(&one);
        let _ = collector.text(&two);
        assert!(collector.finish().is_err());
    }
    for limits in [
        Limits {
            names: 1,
            ..Limits::default()
        },
        Limits {
            name_bytes: 2,
            ..Limits::default()
        },
    ] {
        let mut collector = Collector::open(root.path(), limits).unwrap();
        // The names have no .conf suffix: ignored names still consume bounds.
        assert!(collector.entries(&root.path().join("d")).is_err());
        assert!(collector.finish().is_err());
    }
}

#[test]
fn actual_default_file_and_ignored_name_bounds_are_enforced_and_poisoned() {
    let root = root();
    let cap = Limits::default().file;
    let path = put(root.path(), "a/value", &vec![b'x'; cap]);
    collect(root.path(), |inputs| inputs.text(&path)).unwrap();
    fs::write(&path, vec![b'x'; cap + 1]).unwrap();
    assert!(
        collect(root.path(), |inputs| {
            let _ = inputs.text(&path);
            Ok(())
        })
        .is_err()
    );
    fs::create_dir(root.path().join("a/drop")).unwrap();
    for index in 0..=Limits::default().names {
        put(root.path(), &format!("a/drop/ignored-{index}"), b"");
    }
    assert!(
        collect(root.path(), |inputs| {
            let _ = inputs.entries(&root.path().join("a/drop"));
            Ok(())
        })
        .is_err()
    );
}

#[test]
fn path_component_link_and_repeated_resolution_work_are_bounded() {
    let root = root();
    put(root.path(), "d/value", b"value");
    symlink("second", root.path().join("d/first")).unwrap();
    symlink("value", root.path().join("d/second")).unwrap();
    let mut collector = Collector::open(
        root.path(),
        Limits {
            links: 1,
            ..Limits::default()
        },
    )
    .unwrap();
    assert!(collector.text(&root.path().join("d/first")).is_err());
    assert!(collector.finish().is_err());
    let mut collector = Collector::open(
        root.path(),
        Limits {
            path: root.path().as_os_str().as_bytes().len() + 2,
            ..Limits::default()
        },
    )
    .unwrap();
    assert!(collector.text(&root.path().join("d/value")).is_err());
    assert!(collector.finish().is_err());
    let mut collector = Collector::open(root.path(), Limits::default()).unwrap();
    let too_deep = root
        .path()
        .join(std::iter::repeat_n("d", 65).collect::<Vec<_>>().join("/"));
    assert!(collector.text(&too_deep).is_err());
    assert!(collector.finish().is_err());
    symlink(
        std::iter::repeat_n("d", 63).collect::<Vec<_>>().join("/"),
        root.path().join("d/deep"),
    )
    .unwrap();
    let mut collector = Collector::open(root.path(), Limits::default()).unwrap();
    assert!(collector.text(&root.path().join("d/deep")).is_err());
    assert!(collector.finish().is_err());
    let mut collector = Collector::open(
        root.path(),
        Limits {
            work: 16,
            ..Limits::default()
        },
    )
    .unwrap();
    let path = root.path().join("d/value");
    for _ in 0..20 {
        let _ = collector.text(&path);
    }
    assert!(collector.finish().is_err());
}

#[test]
fn a_body_calibration_change_after_capture_refuses_even_with_carried_base_units() {
    let (root, carried, setting) = fixture(true);
    let units = root.path().join("etc/systemd/system");
    let (_, frozen) = collect(root.path(), |inputs| {
        let mut steps = vec![Step {
            setting,
            act: Act::Write,
        }];
        head_touch::project_with(inputs, root.path(), &units, &carried, &[], &mut steps)?;
        head_touch::verify_with(inputs, root.path(), &units, &carried, &steps)
    })
    .unwrap();
    put(
        root.path(),
        "etc/lampOS/body/lamp.env",
        b"BODY_POSES=\"4,5,6\"\n",
    );
    assert!(frozen.revalidate(root.path()).is_err());
}
