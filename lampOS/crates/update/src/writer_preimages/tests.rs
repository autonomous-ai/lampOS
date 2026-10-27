use super::*;
use crate::{manifest::Setting, writer_settings};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, os::unix::fs::symlink};

const PREFIX: &str = "/etc/lampOS/expression/";
fn transaction() -> String {
    manifest::sha256_hex(b"one fresh private transaction")
}
fn directory() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(Permissions::from_mode(0o700))
        .tempdir()
        .unwrap()
}
fn fixture() -> (
    tempfile::TempDir,
    Vec<Setting>,
    BTreeMap<String, String>,
    BTreeMap<String, String>,
) {
    let root = directory();
    let settings: Vec<_> = [
        "binary",
        "empty",
        "noop",
        "custom",
        "absent/deep",
        "missing",
    ]
    .iter()
    .map(|name| Setting::new(&format!("{PREFIX}{name}"), "new"))
    .collect();
    fs::create_dir_all(root.path().join("etc/lampOS/expression")).unwrap();
    for (index, bytes) in [
        (0, &[0, 255, 17][..]),
        (1, b""),
        (2, b"new"),
        (3, b"private-old"),
    ] {
        fs::write(
            root.path()
                .join(settings[index].path.trim_start_matches('/')),
            bytes,
        )
        .unwrap();
    }
    let recorded = BTreeMap::from([
        (
            settings[0].path.clone(),
            manifest::sha256_hex(&[0, 255, 17]),
        ),
        (settings[2].path.clone(), manifest::sha256_hex(b"new")),
        (settings[4].path.clone(), install::SET_ASIDE.to_owned()),
    ]);
    let writing = BTreeMap::from([(settings[5].path.clone(), manifest::sha256_hex(b"old"))]);
    (root, settings, recorded, writing)
}
fn bundle() -> Bundle {
    let (root, settings, recorded, writing) = fixture();
    let raw = writer_settings::capture(root.path(), &settings, &recorded, &writing).unwrap();
    Bundle::from_capture(&transaction(), &raw).unwrap()
}
fn document() -> Value {
    serde_json::from_slice(bundle().encoded()).unwrap()
}
fn refuses(bytes: &[u8]) -> io::Error {
    let folder = directory();
    let at = folder.path().join(FILE);
    fs::write(&at, bytes).unwrap();
    fs::set_permissions(&at, Permissions::from_mode(0o600)).unwrap();
    let result = Store::open(folder.path())
        .unwrap()
        .load(&transaction(), &Artifact::of(bytes));
    assert_eq!(fs::read(&at).unwrap(), bytes);
    assert!(!folder.path().join(STAGE).exists());
    result.unwrap_err()
}
fn same_observation(a: &Observation, b: &Observation) {
    assert_eq!(a.ancestors, b.ancestors);
    match (&a.entry, &b.entry) {
        (
            Entry::Present {
                bytes: x,
                identity: i,
            },
            Entry::Present {
                bytes: y,
                identity: j,
            },
        ) => {
            assert_eq!(x, y);
            assert_eq!(i, j);
        }
        (Entry::MissingLeaf, Entry::MissingLeaf) => {}
        (Entry::MissingAncestor { component: x }, Entry::MissingAncestor { component: y }) => {
            assert_eq!(x, y)
        }
        _ => panic!("before-image kind changed"),
    }
}

#[test]
fn actual_capture_keeps_binary_empty_missing_noops_records_and_every_identity_after_publication() {
    let (root, settings, recorded, writing) = fixture();
    let raw = writer_settings::capture(root.path(), &settings, &recorded, &writing).unwrap();
    assert!(raw.entries()[2].act().is_none());
    assert!(matches!(
        raw.entries()[3].act(),
        Some(crate::apply::Act::Keep(_))
    ));
    let bundle = Bundle::from_capture(&transaction(), &raw).unwrap();
    let folder = directory();
    let store = Store::open(folder.path()).unwrap();
    let artifact = store.publish(&bundle).unwrap();
    let loaded = store.load(&transaction(), &artifact).unwrap();
    assert_eq!(loaded.encoded(), bundle.encoded());
    assert_eq!(loaded.transaction(), transaction());
    assert_eq!(loaded.entries().len(), settings.len());
    for (old, kept) in raw.entries().iter().zip(loaded.entries()) {
        assert_eq!(old.setting().path, kept.path());
        assert_eq!(old.recorded(), kept.recorded());
        assert_eq!(old.writing(), kept.writing());
        same_observation(old.before(), kept.before());
    }
    assert_eq!(
        fs::metadata(folder.path().join(FILE)).unwrap().mode() & 0o7777,
        0o600
    );
    assert!(!folder.path().join(STAGE).exists());
    assert_eq!(
        fs::read(folder.path().join(FILE)).unwrap(),
        bundle.encoded()
    );
    // No desired text/action is serialized and no live setting is changed.
    let text = std::str::from_utf8(bundle.encoded()).unwrap();
    assert!(!text.contains("\"text\"") && !text.contains("\"act\""));
    raw.revalidate(root.path()).unwrap();
}

#[test]
fn unknown_nonsemantic_fields_retain_the_complete_original_encoding_without_reserialization() {
    let mut doc = document();
    doc["future_note"] = json!({"private-future-value": [1, 2, 3]});
    doc["entries"][0]["future_entry"] = json!(true);
    doc["entries"][0]["before"]["future_before"] = json!([{"later": [1,2,3]}]);
    doc["entries"][0]["ancestors"][0]["future_stamp"] = json!("later");
    let mut bytes = serde_json::to_vec_pretty(&doc).unwrap();
    bytes.extend_from_slice(b" \n\t");
    let artifact = Artifact::of(&bytes);
    let decoded = Bundle::decode(bytes.clone(), &transaction(), &artifact).unwrap();
    let folder = directory();
    let store = Store::open(folder.path()).unwrap();
    store.publish(&decoded).unwrap();
    assert_eq!(
        store.load(&transaction(), &artifact).unwrap().encoded(),
        bytes
    );
}

#[test]
fn expected_identity_and_size_are_checked_before_parse_and_every_failed_load_retains_the_file() {
    let valid = bundle();
    let folder = directory();
    let store = Store::open(folder.path()).unwrap();
    store.publish(&valid).unwrap();
    let before = fs::read(folder.path().join(FILE)).unwrap();
    let mut wrong = valid.artifact().clone();
    wrong.sha256 = "0".repeat(64);
    assert!(store.load(&transaction(), &wrong).is_err());
    wrong = valid.artifact().clone();
    wrong.size += 1;
    assert!(store.load(&transaction(), &wrong).is_err());
    assert!(store.load(&"a".repeat(64), valid.artifact()).is_err());
    assert_eq!(fs::read(folder.path().join(FILE)).unwrap(), before);
    fs::write(folder.path().join(FILE), &before[..before.len() - 1]).unwrap();
    assert!(store.load(&transaction(), valid.artifact()).is_err());
    assert_eq!(
        fs::read(folder.path().join(FILE)).unwrap(),
        &before[..before.len() - 1]
    );
    let file = fs::OpenOptions::new()
        .write(true)
        .open(folder.path().join(FILE))
        .unwrap();
    file.set_len(MOST_BUNDLE_BYTES as u64 + 1).unwrap();
    let mut expected = valid.artifact().clone();
    expected.size = MOST_BUNDLE_BYTES as u64 + 1;
    assert!(store.load(&transaction(), &expected).is_err());
    assert_eq!(file.metadata().unwrap().len(), MOST_BUNDLE_BYTES as u64 + 1);
    let invalid = b"not JSON: private-value".to_vec();
    assert!(
        Bundle::decode(invalid, &transaction(), valid.artifact())
            .unwrap_err()
            .to_string()
            .contains("identity mismatch")
    );
}

#[test]
fn unknown_semantics_known_duplicates_duplicate_paths_and_invalid_records_block_without_deletion() {
    for (field, value) in [
        ("version", json!(2)),
        ("op", json!("future-operation")),
        ("transaction", json!("a".repeat(64))),
    ] {
        let mut doc = document();
        doc[field] = value;
        refuses(&serde_json::to_vec(&doc).unwrap());
    }
    let mut doc = document();
    doc["entries"][0]["before"]["kind"] = json!("future-kind");
    refuses(&serde_json::to_vec(&doc).unwrap());
    let original = String::from_utf8(bundle().encoded().to_owned()).unwrap();
    for (old, duplicate) in [
        ("\"version\":1", "\"version\":1,\"version\":1"),
        ("\"records\":[", "\"records\":[null,null],\"records\":["),
        ("\"device\":", "\"device\":0,\"device\":"),
    ] {
        assert!(original.contains(old));
        refuses(original.replacen(old, duplicate, 1).as_bytes());
    }
    refuses(
        original
            .replacen(
                "\"kind\":\"present\"",
                "\"kind\":\"present\",\"kind\":\"present\"",
                1,
            )
            .as_bytes(),
    );
    let mut doc = document();
    doc["entries"][5]["before"]["identity"] = doc["entries"][0]["before"]["identity"].clone();
    refuses(&serde_json::to_vec(&doc).unwrap());
    let mut doc = document();
    doc["entries"][1]["path"] = doc["entries"][0]["path"].clone();
    refuses(&serde_json::to_vec(&doc).unwrap());
    for path in [
        "/etc/lampOS/update/key.pub",
        "/etc/lampOS/expression/../private",
        "/etc/lampOS/expression/.hidden",
        "/etc/lampOS/expression/a//b",
        "relative",
        "/etc/lampOS/expression/a\u{0}b",
    ] {
        let mut doc = document();
        doc["entries"][0]["path"] = json!(path);
        refuses(&serde_json::to_vec(&doc).unwrap());
    }
    for path in [
        format!("{PREFIX}{}", "a".repeat(512)),
        format!("{PREFIX}{}leaf", "a/".repeat(62)),
    ] {
        let mut doc = document();
        doc["entries"][0]["path"] = json!(path);
        refuses(&serde_json::to_vec(&doc).unwrap());
    }
    for records in [
        json!(["private-value", null]),
        json!([null, install::SET_ASIDE]),
        json!([null]),
    ] {
        let mut doc = document();
        doc["entries"][0]["records"] = records;
        refuses(&serde_json::to_vec(&doc).unwrap());
    }
}

#[test]
fn shape_stamp_base64_and_collection_bounds_are_checked_before_old_byte_allocations() {
    for (field, value) in [
        ("mode", json!(0)),
        ("mode", json!(0o300600)),
        ("links", json!(2)),
        ("length", json!(2)),
        ("length", json!(MOST_FILE_BYTES + 1)),
        ("changed_nanos", json!(1_000_000_000)),
    ] {
        let mut doc = document();
        doc["entries"][0]["before"]["identity"][field] = value;
        refuses(&serde_json::to_vec(&doc).unwrap());
    }
    let mut doc = document();
    doc["entries"][0]["before"]["bytes"] = json!("!!!!");
    refuses(&serde_json::to_vec(&doc).unwrap());
    let mut doc = document();
    doc["entries"][0]["before"]["bytes"] = json!("A".repeat(MOST_BASE64 + 4));
    refuses(&serde_json::to_vec(&doc).unwrap());
    let mut doc = document();
    doc["entries"][1]["ancestors"][0]["inode"] = json!(0);
    refuses(&serde_json::to_vec(&doc).unwrap());
    let mut doc = document();
    doc["entries"][4]["before"]["component"] = json!(0);
    refuses(&serde_json::to_vec(&doc).unwrap());
    let mut doc = document();
    doc["entries"][0]["ancestors"] = json!([]);
    refuses(&serde_json::to_vec(&doc).unwrap());
    let mut doc = document();
    let entry = doc["entries"][0].clone();
    doc["entries"] = Value::Array(vec![entry; 129]);
    refuses(&serde_json::to_vec(&doc).unwrap());
    let mut doc = document();
    let stamp = doc["entries"][0]["ancestors"][0].clone();
    doc["entries"][0]["ancestors"] = Value::Array(vec![stamp; 65]);
    refuses(&serde_json::to_vec(&doc).unwrap());
    // Each claimed file is within cap, but the sum must fail BEFORE base64
    // decoding notices that the deliberately tiny payloads do not match.
    let mut doc = document();
    let mut entries = Vec::new();
    for n in 0..5 {
        let mut entry = doc["entries"][0].clone();
        entry["path"] = json!(format!("{PREFIX}budget{n}"));
        entry["before"]["identity"]["length"] = json!(MOST_FILE_BYTES);
        entries.push(entry);
    }
    doc["entries"] = Value::Array(entries);
    assert_eq!(
        refuses(&serde_json::to_vec(&doc).unwrap()).to_string(),
        "preimage old byte budget exceeded"
    );
}

#[test]
fn publication_never_replaces_existing_final_or_unknown_stage_and_load_never_follows_links() {
    let valid = bundle();
    let witness = directory();
    let outside = witness.path().join("untouched");
    fs::write(&outside, b"outside-private-value").unwrap();
    for name in [FILE, STAGE] {
        for link in [false, true] {
            let folder = directory();
            let at = folder.path().join(name);
            if link {
                symlink(&outside, &at).unwrap();
            } else {
                fs::write(&at, b"retained-private-value").unwrap();
            }
            let store = Store::open(folder.path()).unwrap();
            assert!(store.publish(&valid).is_err());
            assert!(store.load(&transaction(), valid.artifact()).is_err());
            if link {
                assert_eq!(fs::read_link(&at).unwrap(), outside);
            } else {
                assert_eq!(fs::read(&at).unwrap(), b"retained-private-value");
            }
            assert_eq!(fs::read(&outside).unwrap(), b"outside-private-value");
        }
    }
    let public_folder = directory();
    fs::set_permissions(public_folder.path(), Permissions::from_mode(0o755)).unwrap();
    assert!(Store::open(public_folder.path()).is_err());
    assert_eq!(fs::read_dir(public_folder.path()).unwrap().count(), 0);
    let folder = directory();
    let store = Store::open(folder.path()).unwrap();
    store.publish(&valid).unwrap();
    fs::set_permissions(folder.path().join(FILE), Permissions::from_mode(0o644)).unwrap();
    assert!(store.load(&transaction(), valid.artifact()).is_err());
    assert_eq!(fs::read(folder.path().join(FILE)).unwrap(), valid.encoded());
    let folder = directory();
    fs::create_dir(folder.path().join(FILE)).unwrap();
    assert!(
        Store::open(folder.path())
            .unwrap()
            .load(&transaction(), valid.artifact())
            .is_err()
    );
    assert!(folder.path().join(FILE).is_dir());
    let folder = directory();
    let store = Store::open(folder.path()).unwrap();
    store.publish(&valid).unwrap();
    fs::hard_link(folder.path().join(FILE), folder.path().join("other-link")).unwrap();
    assert!(store.load(&transaction(), valid.artifact()).is_err());
    assert_eq!(fs::read(folder.path().join(FILE)).unwrap(), valid.encoded());
    let link = witness.path().join("directory-link");
    symlink(folder.path(), &link).unwrap();
    assert!(Store::open(&link).is_err());
}

#[test]
fn durable_prefix_and_lost_ack_failures_keep_bytes_without_admitting_retry_or_cleanup() {
    let valid = bundle();
    for cut in 0..3 {
        let folder = directory();
        let store = Store::open(folder.path()).unwrap();
        let mut seen = 0;
        assert!(
            store
                .publish_inner(&valid, |_| {
                    let here = seen;
                    seen += 1;
                    if here == cut {
                        Err(io::Error::other("private test acknowledgement failure"))
                    } else {
                        Ok(())
                    }
                })
                .is_err()
        );
        assert_eq!(seen, cut + 1);
        let at = folder.path().join(if cut == 0 { STAGE } else { FILE });
        assert_eq!(fs::read(&at).unwrap(), valid.encoded());
        assert_eq!(fs::metadata(&at).unwrap().mode() & 0o7777, 0o600);
        assert!(store.publish(&valid).is_err());
        assert_eq!(fs::read(&at).unwrap(), valid.encoded());
        if cut == 0 {
            assert!(store.load(&transaction(), valid.artifact()).is_err());
        } else {
            assert_eq!(
                store
                    .load(&transaction(), valid.artifact())
                    .unwrap()
                    .encoded(),
                valid.encoded()
            );
        }
    }
}

#[test]
fn diagnostic_surfaces_hide_values_and_empty_selection_observes_no_live_root() {
    let valid = bundle();
    let debug = format!("{valid:?} {:?}", valid.entries());
    let private_transaction = transaction();
    for private in [PREFIX, "private-old", private_transaction.as_str()] {
        assert!(!debug.contains(private));
    }
    let error = refuses(b"{\"version\":\"private-future-value\"}").to_string();
    assert!(!error.contains("private-future-value"));
    let empty: Vec<Setting> = Vec::new();
    let recorded = BTreeMap::new();
    let writing = BTreeMap::new();
    let raw = writer_settings::capture(
        Path::new("/not-read-private-root"),
        &empty,
        &recorded,
        &writing,
    )
    .unwrap();
    let bundle = Bundle::from_capture(&transaction(), &raw).unwrap();
    assert!(bundle.entries().is_empty());
    let folder = directory();
    let store = Store::open(folder.path()).unwrap();
    store.publish(&bundle).unwrap();
    assert!(
        store
            .load(&transaction(), bundle.artifact())
            .unwrap()
            .entries()
            .is_empty()
    );
}

#[test]
fn exact_four_mib_of_binary_old_bytes_and_empty_files_survive_without_truncation() {
    let root = directory();
    let mut settings = Vec::new();
    fs::create_dir_all(root.path().join("etc/lampOS/expression")).unwrap();
    for n in 0..5 {
        let setting = Setting::new(&format!("{PREFIX}exact{n}"), "new");
        let bytes = if n < 4 {
            vec![n as u8; MOST_FILE_BYTES]
        } else {
            Vec::new()
        };
        fs::write(
            root.path().join(setting.path.trim_start_matches('/')),
            bytes,
        )
        .unwrap();
        settings.push(setting);
    }
    let recorded = BTreeMap::new();
    let writing = BTreeMap::new();
    let raw = writer_settings::capture(root.path(), &settings, &recorded, &writing).unwrap();
    assert_eq!(raw.old_bytes(), MOST_OLD_BYTES);
    let bundle = Bundle::from_capture(&transaction(), &raw).unwrap();
    let loaded = Bundle::decode(
        bundle.encoded().to_owned(),
        &transaction(),
        bundle.artifact(),
    )
    .unwrap();
    for (n, entry) in loaded.entries().iter().enumerate() {
        let Entry::Present { bytes, identity } = &entry.before().entry else {
            panic!("present before-image lost");
        };
        assert_eq!(bytes.len(), if n < 4 { MOST_FILE_BYTES } else { 0 });
        assert!(bytes.iter().all(|byte| *byte == n as u8));
        assert_eq!(identity.length, bytes.len() as u64);
    }
}
