use super::*;
use crate::{
    head_touch::capture::collect,
    manifest::{self, Setting},
    writer_preimages::Bundle,
    writer_refresh::Artifact,
    writer_settings,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::{
        ffi::OsStrExt,
        fs::{PermissionsExt, symlink},
    },
    path::PathBuf,
};

const A: &str = "/etc/lampOS/expression/a";
const B: &str = "/etc/lampOS/expression/b";
const C: &str = "/etc/lampOS/expression/binary";
const PARENT: &str = "/etc/lampOS/expression";

fn root() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap()
}
fn at(root: &Path, path: &str) -> PathBuf {
    root.join(path.trim_start_matches('/'))
}
fn put(root: &Path, path: &str, bytes: &[u8]) {
    let path = at(root, path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}
fn transaction() -> String {
    manifest::sha256_hex(b"private coherence fixture transaction")
}
fn raw_decode(bytes: Vec<u8>) -> Bundle {
    let artifact = Artifact::of(&bytes);
    Bundle::decode(bytes, &transaction(), &artifact)
        .expect("the forged Raw artifact must pass its existing decoder")
}
fn raw(root: &Path, paths: &[&str]) -> Bundle {
    let settings: Vec<_> = paths.iter().map(|path| Setting::new(path, "new")).collect();
    let recorded = BTreeMap::new();
    let writing = BTreeMap::new();
    let captured = writer_settings::capture(root, &settings, &recorded, &writing).unwrap();
    let bundle = Bundle::from_capture(&transaction(), &captured).unwrap();
    raw_decode(bundle.encoded().to_vec())
}
fn changed_raw(bundle: &Bundle, change: impl FnOnce(&mut Value)) -> Bundle {
    let mut doc: Value = serde_json::from_slice(bundle.encoded()).unwrap();
    change(&mut doc);
    raw_decode(serde_json::to_vec(&doc).unwrap())
}
fn frozen(root: &Path, texts: &[&str], directories: &[&str]) -> FrozenInputs {
    collect(root, |inputs| {
        for path in texts {
            inputs.text(&at(root, path))?;
        }
        for path in directories {
            if let Some(entries) = inputs.entries(&at(root, path))? {
                for entry in entries {
                    entry?;
                }
            }
        }
        Ok(())
    })
    .unwrap()
    .1
}
fn head_decode(bytes: &[u8]) -> SavedInputs {
    let sha: [u8; 32] = ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .try_into()
        .unwrap();
    decode(bytes, &sha, bytes.len().try_into().unwrap())
        .expect("the forged Head artifact must pass its existing decoder")
}
fn saved(frozen: &FrozenInputs) -> SavedInputs {
    head_decode(&encode(frozen).unwrap())
}
fn changed_head(frozen: &FrozenInputs, change: impl FnOnce(&mut Portable)) -> SavedInputs {
    let mut wire = Portable::of(frozen).unwrap();
    change(&mut wire);
    head_decode(&serde_json::to_vec(&wire).unwrap())
}
fn agree_unchanged(saved: &SavedInputs, root: &Path, bundle: &Bundle) {
    let head = saved.as_bytes().to_vec();
    let raw = bundle.encoded().to_vec();
    saved.check_preimages(root, bundle).unwrap();
    assert_eq!(saved.as_bytes(), head);
    assert_eq!(bundle.encoded(), raw);
}
fn refuse_unchanged(saved: &SavedInputs, root: &Path, bundle: &Bundle) {
    let head = saved.as_bytes().to_vec();
    let raw = bundle.encoded().to_vec();
    let error = saved.check_preimages(root, bundle).unwrap_err();
    assert_eq!(error, "saved head and preimages disagree");
    assert!(!error.contains("private-coherence-marker"));
    assert_eq!(saved.as_bytes(), head);
    assert_eq!(bundle.encoded(), raw);
}

#[test]
fn decoded_regular_empty_and_unmatched_binary_preserve_artifacts_without_live_io() {
    let root = root();
    put(root.path(), A, b"same");
    put(root.path(), B, b"");
    put(root.path(), C, &[0, 255, 17]);
    let settings = [
        Setting::new(A, "same"),
        Setting::new(B, "new"),
        Setting::new(C, "new"),
    ];
    let recorded = BTreeMap::from([(A.to_owned(), manifest::sha256_hex(b"same"))]);
    let writing = BTreeMap::from([(B.to_owned(), manifest::sha256_hex(b""))]);
    let captured = writer_settings::capture(root.path(), &settings, &recorded, &writing).unwrap();
    assert!(captured.entries()[0].act().is_none());
    let bundle = Bundle::from_capture(&transaction(), &captured).unwrap();
    let bundle = changed_raw(&bundle, |doc| {
        doc["future"] = json!({"marker": "private-coherence-marker"});
    });
    let facts = frozen(root.path(), &[A, B], &[PARENT]);
    let mut head_doc: Value = serde_json::from_slice(&encode(&facts).unwrap()).unwrap();
    head_doc["future"] = json!({"private-coherence-marker": [1, 2, 3]});
    let saved = head_decode(&serde_json::to_vec(&head_doc).unwrap());
    let base = root.path().to_owned();
    drop(captured);
    root.close().unwrap();
    agree_unchanged(&saved, &base, &bundle);
    assert_eq!(
        bundle.entries()[0].recorded(),
        recorded.get(A).map(String::as_str)
    );
    assert_eq!(
        bundle.entries()[1].writing(),
        writing.get(B).map(String::as_str)
    );
}

#[test]
fn both_typed_absences_agree_at_each_first_missing_component() {
    let parts: Vec<_> = A.trim_start_matches('/').split('/').collect();
    for first in 0..parts.len() {
        let root = root();
        if first != 0 {
            fs::create_dir_all(root.path().join(parts[..first].join("/"))).unwrap();
        }
        let bundle = raw(root.path(), &[A]);
        let facts = frozen(root.path(), &[A], &[A]);
        let saved = saved(&facts);
        match &bundle.entries()[0].before().entry {
            crate::writer_files::Entry::MissingAncestor { component } => {
                assert_eq!(*component, first)
            }
            crate::writer_files::Entry::MissingLeaf => assert_eq!(first + 1, parts.len()),
            _ => panic!("fixture unexpectedly contains the requested file"),
        }
        agree_unchanged(&saved, root.path(), &bundle);
    }
}

#[test]
fn exact_root_spelling_and_nonempty_raw_root_evidence_are_required() {
    let root = root();
    put(root.path(), A, b"same");
    let bundle = raw(root.path(), &[A]);
    let saved = saved(&frozen(root.path(), &[], &[]));
    // An uncaptured Raw leaf is permitted, but its root still supplies evidence.
    agree_unchanged(&saved, root.path(), &bundle);
    for alternate in [
        root.path().join("."),
        PathBuf::from(format!("{}/", root.path().display())),
        root.path().join("other"),
    ] {
        refuse_unchanged(&saved, &alternate, &bundle);
    }
    let empty = raw(root.path(), &[]);
    assert!(empty.entries().is_empty());
    refuse_unchanged(&saved, root.path(), &empty);
}

#[test]
fn every_raw_entry_checks_common_ancestors_even_without_a_matching_head_leaf() {
    let root = root();
    put(root.path(), A, b"same");
    put(root.path(), B, b"private-coherence-marker");
    let bundle = raw(root.path(), &[A, B]);
    let saved = saved(&frozen(root.path(), &[A], &[]));
    agree_unchanged(&saved, root.path(), &bundle);
    // The first row and root still match. Only the uncaptured second row's
    // represented common parent changed; both original decoders accept it.
    let changed = changed_raw(&bundle, |doc| {
        let stamp = &mut doc["entries"][1]["ancestors"][3];
        let prior = stamp["changed_nanos"].as_i64().unwrap();
        stamp["changed_nanos"] = json!((prior + 1) % 1_000_000_000);
    });
    refuse_unchanged(&saved, root.path(), &changed);
    let changed_root = changed_raw(&bundle, |doc| {
        for entry in doc["entries"].as_array_mut().unwrap() {
            let prior = entry["ancestors"][0]["mode"].as_u64().unwrap();
            entry["ancestors"][0]["mode"] = json!(prior ^ 0o010);
        }
    });
    refuse_unchanged(&saved, root.path(), &changed_root);
}

#[test]
fn common_leaf_fields_and_exact_binary_bytes_cannot_be_substituted() {
    let root = root();
    put(root.path(), A, b"same");
    let bundle = raw(root.path(), &[A]);
    let facts = frozen(root.path(), &[A], &[]);
    let changes: [fn(&mut Stamp); 10] = [
        |s| s.dev += 1,
        |s| s.ino += 1,
        |s| s.mode ^= 0o100,
        |s| s.nlink += 1,
        |s| s.uid = s.uid.wrapping_add(1),
        |s| s.gid = s.gid.wrapping_add(1),
        |s| s.mtime += 1,
        |s| s.mtime_ns = (s.mtime_ns + 1) % 1_000_000_000,
        |s| s.ctime += 1,
        |s| s.ctime_ns = (s.ctime_ns + 1) % 1_000_000_000,
    ];
    for change in changes {
        let saved = changed_head(&facts, |wire| {
            let SavedText::Regular { stamp, .. } = &mut wire.texts[0].fact else {
                unreachable!()
            };
            change(stamp);
        });
        refuse_unchanged(&saved, root.path(), &bundle);
    }
    for grow in [false, true] {
        let saved = changed_head(&facts, |wire| {
            let SavedText::Regular { text, stamp } = &mut wire.texts[0].fact else {
                unreachable!()
            };
            if grow {
                text.push('!');
                stamp.size += 1;
            } else {
                *text = "diff".into();
            }
        });
        refuse_unchanged(&saved, root.path(), &bundle);
    }
    // Raw has no rdev field: never manufacture an rdev=0 claim for it.
    let extra_head_field = changed_head(&facts, |wire| {
        let SavedText::Regular { stamp, .. } = &mut wire.texts[0].fact else {
            unreachable!()
        };
        stamp.rdev = 17;
    });
    agree_unchanged(&extra_head_field, root.path(), &bundle);
}

#[test]
fn signed_device_conversion_refuses_negative_instead_of_reinterpreting_bits() {
    let root = root();
    put(root.path(), A, b"same");
    let original = raw(root.path(), &[A]);
    let bundle = changed_raw(&original, |doc| {
        doc["entries"][0]["ancestors"][0]["device"] = json!(u64::MAX);
    });
    let facts = frozen(root.path(), &[A], &[]);
    let with_device = |device| {
        changed_head(&facts, |wire| {
            wire.root_stamp.dev = device;
            for row in &mut wire.texts {
                let SavedWitness::Directory(stamp) = &mut row.witnesses[0] else {
                    unreachable!()
                };
                stamp.dev = device;
            }
        })
    };
    agree_unchanged(&with_device(i128::from(u64::MAX)), root.path(), &bundle);
    // Both decoders accept these numbers, but a lossy bitcast would mistake
    // negative dev_t for the Raw u64::MAX object identity.
    refuse_unchanged(&with_device(-1), root.path(), &bundle);
}

#[test]
fn negative_kind_suffix_and_no_link_chain_shape_match_the_requested_key() {
    let root = root();
    fs::create_dir(root.path().join("etc")).unwrap();
    let bundle = raw(root.path(), &[A]);
    let facts = frozen(root.path(), &[A], &[]);
    for alteration in 0..3 {
        let saved = changed_head(&facts, |wire| match alteration {
            0 => {
                let SavedText::MissingAncestor { suffix } = &mut wire.texts[0].fact else {
                    unreachable!()
                };
                suffix.last_mut().unwrap().0 = b"other".to_vec();
            }
            1 => wire.texts[0].fact = SavedText::MissingLeaf,
            _ => {
                let extra = wire.texts[0].witnesses.last().unwrap().clone();
                wire.texts[0].witnesses.push(extra);
            }
        });
        refuse_unchanged(&saved, root.path(), &bundle);
    }
}

#[test]
fn unrelated_head_links_and_null_masks_are_valid_but_raw_overlap_refuses() {
    let root = root();
    put(root.path(), A, b"same");
    put(root.path(), "/etc/other/target", b"other");
    symlink("target", at(root.path(), "/etc/other/link")).unwrap();
    symlink("/dev/null", at(root.path(), "/etc/other/null")).unwrap();
    let bundle = raw(root.path(), &[A]);
    let facts = frozen(root.path(), &[A, "/etc/other/link", "/etc/other/null"], &[]);
    agree_unchanged(&saved(&facts), root.path(), &bundle);
    let key = at(root.path(), A);
    for null in [false, true] {
        let altered = changed_head(&facts, |wire| {
            let extra_link = wire
                .texts
                .iter()
                .flat_map(|r| &r.witnesses)
                .find(|w| matches!(w, SavedWitness::Link { .. }))
                .unwrap()
                .clone();
            let mask = wire
                .texts
                .iter()
                .find(|r| matches!(&r.fact, SavedText::NullMask { .. }))
                .unwrap()
                .fact
                .clone();
            wire.texts
                .retain(|r| r.path.0 == key.as_os_str().as_bytes());
            assert_eq!(wire.texts.len(), 1);
            if null {
                wire.texts[0].fact = mask;
            } else {
                wire.texts[0].witnesses.push(extra_link);
            }
        });
        refuse_unchanged(&altered, root.path(), &bundle);
    }
}

#[test]
fn directory_facts_check_types_membership_absence_and_extra_link_bypass() {
    let root = root();
    put(root.path(), A, b"same");
    put(root.path(), "/etc/other/target", b"other");
    symlink("target", at(root.path(), "/etc/other/link")).unwrap();
    fs::create_dir(at(root.path(), "/etc/lampOS/expression/dir")).unwrap();
    let present = raw(root.path(), &[A]);
    let facts = frozen(
        root.path(),
        &["/etc/other/link"],
        &[PARENT, "/etc/lampOS/expression/dir"],
    );
    let parent = at(root.path(), PARENT);
    for extra_link in [false, true] {
        let changed = changed_head(&facts, |wire| {
            let link = wire
                .texts
                .iter()
                .flat_map(|r| &r.witnesses)
                .find(|w| matches!(w, SavedWitness::Link { .. }))
                .unwrap()
                .clone();
            let row = wire
                .directories
                .iter_mut()
                .find(|r| r.path.0 == parent.as_os_str().as_bytes())
                .unwrap();
            row.names.as_mut().unwrap().retain(|n| n.0 != b"a");
            if extra_link {
                row.witnesses.push(link);
            }
        });
        // Extra link is past this DirectoryRow's lexical leaf. The bounded
        // decoder accepts it; plan mapping must not use the link shortcut to
        // bypass the contradictory membership (the pre-gate P2 regression).
        refuse_unchanged(&changed, root.path(), &present);
    }
    let key = at(root.path(), A);
    let directory_as_file = changed_head(&facts, |wire| {
        wire.texts.clear();
        wire.directories
            .retain(|r| r.path.0 != parent.as_os_str().as_bytes());
        assert_eq!(wire.directories.len(), 1);
        wire.directories[0].path.0 = key.as_os_str().as_bytes().to_vec();
    });
    refuse_unchanged(&directory_as_file, root.path(), &present);
    // The same path can be absent as both a file request and directory request;
    // a present empty directory and a missing file are never interchangeable.
    let missing = raw(root.path(), &[B]);
    let missing_key = at(root.path(), B);
    let directory_at_missing_file = changed_head(&facts, |wire| {
        wire.texts.clear();
        wire.directories
            .retain(|r| r.path.0 != parent.as_os_str().as_bytes());
        wire.directories[0].path.0 = missing_key.as_os_str().as_bytes().to_vec();
    });
    refuse_unchanged(&directory_at_missing_file, root.path(), &missing);
    let a_text = frozen(root.path(), &[A], &[]);
    let ancestor_as_regular_file = changed_head(&a_text, |wire| {
        wire.texts[0].path.0 = at(root.path(), "/etc").as_os_str().as_bytes().to_vec();
        wire.texts[0].witnesses.truncate(1);
    });
    refuse_unchanged(&ancestor_as_regular_file, root.path(), &present);
    let negatives = frozen(root.path(), &[B], &[B, PARENT]);
    agree_unchanged(&saved(&negatives), root.path(), &missing);
    let invented_member = changed_head(&negatives, |wire| {
        let row = wire
            .directories
            .iter_mut()
            .find(|r| r.path.0 == parent.as_os_str().as_bytes())
            .unwrap();
        row.names.as_mut().unwrap().push(Raw(b"b".to_vec()));
    });
    refuse_unchanged(&invented_member, root.path(), &missing);
}

#[test]
fn unchanged_bytes_with_real_permission_changes_fail_without_snapshot_or_auth_claims() {
    let root = root();
    put(root.path(), A, b"same");
    let bundle = raw(root.path(), &[A]);
    let path = at(root.path(), A);
    let before = fs::metadata(&path).unwrap().permissions().mode();
    fs::set_permissions(&path, fs::Permissions::from_mode(before ^ 0o100)).unwrap();
    let facts = frozen(root.path(), &[A], &[]);
    refuse_unchanged(&saved(&facts), root.path(), &bundle);
    // An entirely uncaptured leaf still cannot skip the Raw root comparison.
    let before_root = fs::metadata(root.path()).unwrap().permissions().mode();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(before_root ^ 0o010)).unwrap();
    let facts = frozen(root.path(), &[], &[]);
    refuse_unchanged(&saved(&facts), root.path(), &bundle);
}
