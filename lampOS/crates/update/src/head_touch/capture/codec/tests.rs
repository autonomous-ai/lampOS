use super::super::collect;
use super::*;
use crate::{
    apply::{Act, Step},
    head_touch,
    manifest::Setting,
};
use std::{
    cell::Cell,
    collections::BTreeMap,
    ffi::OsString,
    fs,
    os::unix::{
        ffi::OsStringExt,
        fs::{PermissionsExt, symlink},
    },
    path::PathBuf,
};

fn root() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap()
}
fn put(root: &Path, name: &str, value: &[u8]) -> PathBuf {
    let path = root.join(name.trim_start_matches('/'));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, value).unwrap();
    path
}
fn sha(bytes: &[u8]) -> [u8; 32] {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .try_into()
        .unwrap()
}
fn open(bytes: &[u8]) -> Result<SavedInputs> {
    decode(bytes, &sha(bytes), bytes.len().try_into().unwrap())
}
fn change(frozen: &FrozenInputs, mutate: impl FnOnce(&mut Portable)) -> Vec<u8> {
    let mut wire = Portable::of(frozen).unwrap();
    mutate(&mut wire);
    serde_json::to_vec(&wire).unwrap()
}
fn one() -> (tempfile::TempDir, PathBuf, FrozenInputs) {
    let root = root();
    let path = put(root.path(), "value", b"safe");
    let (_, frozen) = collect(root.path(), |p| p.text(&path)).unwrap();
    (root, path, frozen)
}
fn append_future(bytes: &[u8], field: &[u8]) -> Vec<u8> {
    let mut result = bytes[..bytes.len() - 1].to_vec();
    result.push(b',');
    result.extend_from_slice(field);
    result.push(b'}');
    result
}

#[test]
fn closed_restart_keeps_regular_empty_null_and_both_absences_distinct() {
    let root = root();
    let base = root.path().to_owned();
    let regular = put(&base, "value", b"a\0line\n\t\r\x7f");
    let empty = put(&base, "empty", b"");
    let null = base.join("null");
    symlink("/dev/null", &null).unwrap();
    let leaf = base.join("missing");
    let ancestor = base.join("gone/value");
    let empty_dir = base.join("empty-dir");
    fs::create_dir(&empty_dir).unwrap();
    let (_, frozen) = collect(&base, |p| {
        for path in [&regular, &empty, &null, &leaf, &ancestor] {
            p.text(path)?;
        }
        p.entries(&leaf)?;
        p.entries(&empty_dir)?;
        Ok(())
    })
    .unwrap();
    let bytes = encode(&frozen).unwrap();
    assert_eq!(bytes, encode(&frozen).unwrap());
    root.close().unwrap();
    let mut saved = open(&bytes).unwrap();
    assert_eq!(
        saved.text(&regular).unwrap().as_deref(),
        Some("a\0line\n\t\r\x7f")
    );
    assert_eq!(saved.text(&empty).unwrap().as_deref(), Some(""));
    assert_eq!(saved.text(&null).unwrap().as_deref(), Some(""));
    assert_eq!(saved.text(&leaf).unwrap(), None);
    assert_eq!(saved.text(&ancestor).unwrap(), None);
    assert!(saved.entries(&leaf).unwrap().is_none());
    assert_eq!(saved.entries(&empty_dir).unwrap().unwrap().count(), 0);
    assert!(saved.text(&base.join("uncaptured")).is_err());
    assert!(saved.entries(&regular).is_err());
    assert_eq!(saved.as_bytes(), bytes);
    assert!(saved.revalidate(&base).is_err());
    let null_row = saved
        .facts
        .texts
        .iter()
        .find(|r| r.path.0 == null.as_os_str().as_bytes())
        .unwrap();
    assert!(matches!(null_row.fact, SavedText::NullMask { .. }));
    let empty_row = saved
        .facts
        .texts
        .iter()
        .find(|r| r.path.0 == empty.as_os_str().as_bytes())
        .unwrap();
    assert!(matches!(empty_row.fact, SavedText::Regular { .. }));
}

#[test]
fn relative_aliases_absolute_contained_links_and_raw_unresolved_suffix_replay() {
    let root = root();
    let base = root.path().to_owned();
    let file = put(&base, "real/value", b"kept");
    symlink("real", base.join("alias")).unwrap();
    symlink(&file, base.join("absolute")).unwrap();
    symlink("not-there/../tail", base.join("dangling")).unwrap();
    let alias = base.join("alias/value");
    let absolute = base.join("absolute");
    let missing = base.join("dangling/value");
    let (_, frozen) = collect(&base, |p| {
        p.text(&alias)?;
        p.text(&absolute)?;
        p.text(&missing)?;
        p.entries(&base.join("alias"))?;
        Ok(())
    })
    .unwrap();
    let bytes = encode(&frozen).unwrap();
    let mut saved = open(&bytes).unwrap();
    saved.revalidate(&base).unwrap();
    root.close().unwrap();
    assert_eq!(saved.text(&alias).unwrap().as_deref(), Some("kept"));
    assert_eq!(saved.text(&absolute).unwrap().as_deref(), Some("kept"));
    assert_eq!(saved.text(&missing).unwrap(), None);
    let row = saved
        .facts
        .texts
        .iter()
        .find(|r| r.path.0 == missing.as_os_str().as_bytes())
        .unwrap();
    let SavedText::MissingAncestor { suffix } = &row.fact else {
        panic!("missing ancestor required");
    };
    assert!(suffix.iter().any(|p| p.0 == b".."));
    assert_eq!(
        saved
            .entries(&base.join("alias"))
            .unwrap()
            .unwrap()
            .map(|p| p.unwrap())
            .collect::<Vec<_>>(),
        vec![alias]
    );
}

#[test]
fn portable_raw_names_and_link_bytes_are_exact_without_a_physical_mac_claim() {
    let root = root();
    let directory = root.path().join("drop");
    fs::create_dir(&directory).unwrap();
    let file = put(root.path(), "value", b"safe");
    symlink("value", root.path().join("link")).unwrap();
    let (_, mut frozen) = collect(root.path(), |p| {
        p.entries(&directory)?;
        p.text(&root.path().join("link"))?;
        Ok(())
    })
    .unwrap();
    // Controlled wire-only mutation, not an admitted physical snapshot.
    let names = vec![
        OsString::from_vec(vec![b'x', 0xff]),
        OsString::from("ignored.txt"),
    ];
    frozen.directories.get_mut(&directory).unwrap().names = Some(names.clone());
    let fact = frozen.texts.get_mut(&root.path().join("link")).unwrap();
    let Witness::Link { target, .. } = fact
        .witnesses
        .iter_mut()
        .find(|w| matches!(w, Witness::Link { .. }))
        .unwrap()
    else {
        unreachable!()
    };
    *target = vec![b'v', 0xff, b'l', b'u', b'e'];
    let bytes = encode(&frozen).unwrap();
    root.close().unwrap();
    let mut saved = open(&bytes).unwrap();
    let got = saved
        .entries(&directory)
        .unwrap()
        .unwrap()
        .map(|p| p.unwrap().file_name().unwrap().to_os_string())
        .collect::<Vec<_>>();
    assert_eq!(got, names);
    assert!(saved.facts.texts[0].witnesses.iter().any(
        |w| matches!(w, SavedWitness::Link { target, .. } if target.0 == [b'v',0xff,b'l',b'u',b'e'])
    ));
    assert!(saved.text(&file).is_err());
    #[cfg(target_os = "linux")]
    {
        let physical = self::root();
        let raw = physical.path().join(OsString::from_vec(vec![b'x', 0xff]));
        fs::write(&raw, b"native").unwrap();
        let (_, facts) = collect(physical.path(), |p| {
            p.entries(physical.path())?;
            p.text(&raw)
        })
        .unwrap();
        let bytes = encode(&facts).unwrap();
        let physical_root = physical.path().to_owned();
        physical.close().unwrap();
        let mut saved = open(&bytes).unwrap();
        assert_eq!(saved.text(&raw).unwrap().as_deref(), Some("native"));
        assert_eq!(
            saved
                .entries(&physical_root)
                .unwrap()
                .unwrap()
                .next()
                .unwrap()
                .unwrap(),
            raw
        );
    }
}

#[test]
fn every_native_equality_field_is_selected_and_atime_is_excluded() {
    let (_root, _path, frozen) = one();
    let initial = frozen.root_identity;
    let changes: &[fn(&mut Identity)] = &[
        |s| s.0.st_dev += 1,
        |s| s.0.st_ino += 1,
        |s| s.0.st_mode ^= 1,
        |s| s.0.st_nlink += 1,
        |s| s.0.st_uid += 1,
        |s| s.0.st_gid += 1,
        |s| s.0.st_rdev += 1,
        |s| s.0.st_size += 1,
        |s| s.0.st_mtime -= 1,
        |s| s.0.st_mtime_nsec = (s.0.st_mtime_nsec + 1) % 1_000_000_000,
        |s| s.0.st_ctime -= 1,
        |s| s.0.st_ctime_nsec = (s.0.st_ctime_nsec + 1) % 1_000_000_000,
    ];
    for alter in changes {
        let mut other = initial;
        alter(&mut other);
        assert!(initial != other);
        assert!(Stamp::of(initial).unwrap() != Stamp::of(other).unwrap());
    }
    let mut atime = initial;
    atime.0.st_atime -= 1;
    assert!(initial == atime);
    assert!(Stamp::of(initial).unwrap() == Stamp::of(atime).unwrap());
    atime.0.st_atime_nsec = (atime.0.st_atime_nsec + 1) % 1_000_000_000;
    assert!(initial == atime);
    assert!(Stamp::of(initial).unwrap() == Stamp::of(atime).unwrap());
}

#[test]
fn signed_timestamps_and_portable_widths_refuse_unsupported_values() {
    let (_root, _path, frozen) = one();
    let mut stamp = Stamp::of(frozen.root_identity).unwrap();
    stamp.mtime = -1;
    stamp.ctime = i64::MIN;
    stamp.dev = -2_147_483_648;
    stamp.rdev = 18_446_744_073_709_551_615;
    let bytes = serde_json::to_vec(&stamp).unwrap();
    lexical_bounds(&bytes).unwrap();
    let restored: Stamp = serde_json::from_slice(&bytes).unwrap();
    assert!(restored == stamp);
    restored.check().unwrap();
    for mode in [0x1_0000_u32, u32::MAX] {
        let mut bad = stamp.clone();
        bad.mode |= mode;
        assert!(bad.check().is_err());
    }
    let mut bad = stamp.clone();
    bad.mtime_ns = 1_000_000_000;
    assert!(bad.check().is_err());
    let mut bad = stamp;
    bad.dev = 18_446_744_073_709_551_616;
    assert!(bad.check().is_err());
}

#[test]
fn exact_external_hash_and_size_are_checked_and_do_not_authenticate_origin() {
    let (_root, _path, frozen) = one();
    let bytes = encode(&frozen).unwrap();
    assert!(decode(&bytes, &[0; 32], bytes.len().try_into().unwrap()).is_err());
    assert!(decode(&bytes, &sha(&bytes), (bytes.len() + 1).try_into().unwrap()).is_err());
    let mut changed = bytes.clone();
    changed.push(b' ');
    assert!(decode(&changed, &sha(&bytes), changed.len().try_into().unwrap()).is_err());
    assert!(open(&changed).is_ok()); // Exact selected bytes can include JSON whitespace.
    let mut changed = bytes;
    changed.extend_from_slice(b" {}");
    assert!(open(&changed).is_err());
}

#[test]
fn future_nonsemantic_bytes_and_escape_spellings_are_retained_exactly() {
    let (root, _path, frozen) = one();
    let bytes = encode(&frozen).unwrap();
    let future = append_future(
        &bytes,
        br#""fu\u0074ure":{"quoted":"a\"b\\c{[}]","nested":[null,true,17,{"note":"kept"}]}"#,
    );
    let mut padded = b" \n\t".to_vec();
    padded.extend_from_slice(&future);
    padded.extend_from_slice(b"\r\n ");
    let saved = open(&padded).unwrap();
    assert_eq!(saved.as_bytes(), padded);
    saved.revalidate(root.path()).unwrap();
    let debug = format!("{saved:?}");
    assert!(!debug.contains("quoted"));
    assert!(!debug.contains("kept"));
    assert!(!debug.contains(root.path().to_str().unwrap()));
    // Byte UTF-8 must be checked even for fields serde would otherwise skip.
    let invalid = append_future(&bytes, b"\"future\":\"\xff\"");
    assert!(open(&invalid).is_err());
}

#[test]
fn semantic_versions_ops_kinds_duplicates_and_nullable_shadowing_refuse() {
    let (_root, _path, frozen) = one();
    let bytes = encode(&frozen).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    for (before, after) in [
        (r#""schema":1"#, r#""schema":2"#),
        (r#""op":"head_inputs""#, r#""op":"future""#),
        (r#""kind":"regular""#, r#""kind":"future""#),
        (r#""schema":1"#, r#""schema":1,"sch\u0065ma":1"#),
        (r#""dev":"#, r#""dev":0,"dev":"#),
    ] {
        assert!(text.contains(before));
        assert!(open(text.replace(before, after).as_bytes()).is_err());
    }
    let root = root();
    let missing = root.path().join("missing");
    let (_, frozen) = collect(root.path(), |p| p.entries(&missing).map(|_| ())).unwrap();
    let text = String::from_utf8(encode(&frozen).unwrap()).unwrap();
    assert!(
        open(
            text.replace(r#""names":null"#, r#""names":null,"names":[]"#)
                .as_bytes()
        )
        .is_err()
    );
    assert!(
        open(
            text.replace(
                r#""kind":"missing_leaf""#,
                r#""kind":"missing_leaf","kind":"missing_leaf""#
            )
            .as_bytes()
        )
        .is_err()
    );
}

#[test]
fn malformed_strings_truncations_and_non_utf8_regular_bytes_never_yield_partial_saved() {
    let (_root, _path, frozen) = one();
    let bytes = encode(&frozen).unwrap();
    for end in 0..bytes.len() {
        assert!(open(&bytes[..end]).is_err());
    }
    for field in [
        br#""future":"bad\q""#.as_slice(),
        b"\"future\":\"unescaped\nline\"",
        br#""future":[1,]"#,
    ] {
        assert!(open(&append_future(&bytes, field)).is_err());
    }
    let source = String::from_utf8(bytes).unwrap();
    let invalid = source.replace(
        &format!("\"{}\"", STANDARD.encode(b"safe")),
        &format!("\"{}\"", STANDARD.encode([0xff; 4])),
    );
    assert!(invalid != source);
    assert!(open(invalid.as_bytes()).is_err());
}

#[test]
fn joined_name_matches_the_native_exact_limit_and_refuses_one_over() {
    let root = root();
    let file = put(root.path(), "drop/x", b"");
    let directory = file.parent().unwrap();
    let cap = directory.as_os_str().as_bytes().len() + 2;
    let mut producer = Collector::open(
        root.path(),
        Limits {
            path: cap,
            ..WIRE_LIMITS
        },
    )
    .unwrap();
    assert_eq!(
        producer
            .entries(directory)
            .unwrap()
            .unwrap()
            .next()
            .unwrap()
            .unwrap(),
        file
    );
    let mut frozen = producer.finish().unwrap();
    let bytes = encode(&frozen).unwrap();
    let mut saved = open(&bytes).unwrap();
    let got = saved
        .entries(directory)
        .unwrap()
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    assert_eq!(got.as_os_str().as_bytes().len(), cap);
    let bad = change(&frozen, |p| {
        p.directories[0].names.as_mut().unwrap()[0] = Raw(b"xx".to_vec())
    });
    assert!(open(&bad).is_err());
    frozen.directories.get_mut(directory).unwrap().names = Some(vec![OsString::from("xx")]);
    assert!(encode(&frozen).is_err());
}

#[test]
fn same_absent_key_is_coherent_across_both_apis_and_conflicts_refuse() {
    let root = root();
    let path = root.path().join("gone");
    let (_, frozen) = collect(root.path(), |p| {
        p.text(&path)?;
        p.entries(&path)?;
        Ok(())
    })
    .unwrap();
    let mut saved = open(&encode(&frozen).unwrap()).unwrap();
    assert_eq!(saved.text(&path).unwrap(), None);
    assert!(saved.entries(&path).unwrap().is_none());
    let conflicting = change(&frozen, |p| {
        p.directories[0].names = Some(Vec::new());
        p.directories[0].missing = None;
    });
    assert!(open(&conflicting).is_err());
    let conflicting = change(&frozen, |p| {
        p.directories[0].missing = Some(SavedText::MissingAncestor {
            suffix: vec![Raw(b"a".to_vec()), Raw(b"b".to_vec())],
        })
    });
    assert!(open(&conflicting).is_err());
}

#[test]
fn sorted_unique_rooted_keys_and_root_witness_agreement_are_required() {
    let root = root();
    let a = put(root.path(), "a", b"a");
    let b = put(root.path(), "b", b"b");
    let (_, frozen) = collect(root.path(), |p| {
        p.text(&a)?;
        p.text(&b)?;
        Ok(())
    })
    .unwrap();
    assert!(open(&change(&frozen, |p| p.texts.swap(0, 1))).is_err());
    assert!(open(&change(&frozen, |p| p.texts[1].path = p.texts[0].path.clone())).is_err());
    assert!(
        open(&change(&frozen, |p| p.texts[0].path =
            Raw(b"/elsewhere/value".to_vec())))
        .is_err()
    );
    assert!(
        open(&change(&frozen, |p| p.texts[0].path =
            Raw(format!("{}/./a", root.path().display()).into_bytes())))
        .is_err()
    );
    assert!(
        open(&change(&frozen, |p| {
            let SavedWitness::Directory(s) = &mut p.texts[0].witnesses[0] else {
                unreachable!()
            };
            s.ino += 1;
        }))
        .is_err()
    );
}

#[test]
fn quota_refusal_happens_before_a_base64_allocation_is_invoked() {
    for (kind, budget) in [
        (
            BytesKind::Text,
            Budget {
                text: WIRE_LIMITS.bytes,
                ..Budget::default()
            },
        ),
        (
            BytesKind::Raw,
            Budget {
                raw: RAW_BYTES,
                ..Budget::default()
            },
        ),
        (
            BytesKind::Name,
            Budget {
                names: NAMES,
                ..Budget::default()
            },
        ),
        (
            BytesKind::Name,
            Budget {
                name_bytes: WIRE_LIMITS.name_bytes,
                ..Budget::default()
            },
        ),
        (
            BytesKind::Suffix,
            Budget {
                suffix: WIRE_LIMITS.path * (WIRE_LIMITS.links + 1),
                ..Budget::default()
            },
        ),
    ] {
        let called = Cell::new(false);
        let mut budget = budget;
        let result = decode_raw_with("YQ==", kind, WIRE_LIMITS, &mut budget, |n| {
            called.set(true);
            allocation(n)
        });
        assert!(result.is_err());
        assert!(!called.get());
    }
    let called = Cell::new(false);
    let mut budget = Budget::default();
    let too_large = "YWFh".repeat(WIRE_LIMITS.file / 3 + 1);
    assert!(
        decode_raw_with(&too_large, BytesKind::Text, WIRE_LIMITS, &mut budget, |n| {
            called.set(true);
            allocation(n)
        })
        .is_err()
    );
    assert!(!called.get());
}

struct ForbiddenDeserializer;
impl<'de> Deserializer<'de> for ForbiddenDeserializer {
    type Error = serde::de::value::Error;
    fn deserialize_any<V: Visitor<'de>>(self, _: V) -> std::result::Result<V::Value, Self::Error> {
        panic!("quota must refuse before entering a nested collection")
    }
    serde::forward_to_deserialize_any! { bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf option unit unit_struct newtype_struct seq tuple tuple_struct map struct enum identifier ignored_any }
}

#[test]
fn fact_and_witness_quotas_refuse_before_deserializing_the_next_collection() {
    let (_root, _path, frozen) = one();
    let p = Portable::of(&frozen).unwrap();
    let header = Header {
        root: p.root,
        stamp: p.root_stamp,
        limits: p.limits,
    };
    let mut budget = Budget {
        facts: FACTS,
        ..Budget::default()
    };
    assert!(
        TextRowSeed {
            budget: &mut budget,
            header: &header
        }
        .deserialize(ForbiddenDeserializer)
        .is_err()
    );
    assert!(
        DirectoryRowSeed {
            budget: &mut budget,
            header: &header
        }
        .deserialize(ForbiddenDeserializer)
        .is_err()
    );
    let mut budget = Budget {
        witnesses: WITNESSES,
        ..Budget::default()
    };
    assert!(
        WitnessSeed {
            budget: &mut budget,
            limits: WIRE_LIMITS,
            allowed: true
        }
        .deserialize(ForbiddenDeserializer)
        .is_err()
    );
    let mut budget = Budget::default();
    assert!(
        WitnessSeed {
            budget: &mut budget,
            limits: WIRE_LIMITS,
            allowed: false
        }
        .deserialize(ForbiddenDeserializer)
        .is_err()
    );
}

#[test]
fn root_slash_retains_the_producers_conservative_extra_separator_cap() {
    let (_private_root, _path, mut frozen) = one();
    // Controlled format fixture only: this does not capture the host '/'.
    frozen.root_path = PathBuf::from("/");
    frozen.texts.clear();
    frozen.directories.clear();
    frozen.limits.path = 3;
    frozen.directories.insert(
        PathBuf::from("/"),
        DirectoryFact {
            witnesses: vec![Witness::Directory(frozen.root_identity)],
            names: Some(vec![OsString::from("x")]),
            missing: None,
        },
    );
    let mut saved = open(&encode(&frozen).unwrap()).unwrap();
    assert_eq!(
        saved
            .entries(Path::new("/"))
            .unwrap()
            .unwrap()
            .next()
            .unwrap()
            .unwrap(),
        PathBuf::from("/x")
    );
    let bad = change(&frozen, |p| p.limits.path = 2);
    assert!(open(&bad).is_err());
    frozen.limits.path = 2;
    assert!(encode(&frozen).is_err());
}

#[test]
fn root_is_not_a_text_or_missing_directory_and_has_one_exact_witness() {
    let (root, path, mut frozen) = one();
    let bad = change(&frozen, |p| p.texts[0].path = p.root.clone());
    assert!(open(&bad).is_err());
    let bad = change(&frozen, |p| {
        p.texts[0].path = p.root.clone();
        p.texts[0].fact = SavedText::MissingLeaf;
    });
    assert!(open(&bad).is_err());
    let fact = frozen.texts.remove(&path).unwrap();
    frozen.texts.insert(root.path().to_owned(), fact);
    assert!(encode(&frozen).is_err());
    let (_, mut directory) = collect(root.path(), |p| p.entries(root.path()).map(|_| ())).unwrap();
    assert!(open(&encode(&directory).unwrap()).is_ok());
    let bad = change(&directory, |p| {
        p.directories[0].names = None;
        p.directories[0].missing = Some(SavedText::MissingLeaf);
    });
    assert!(open(&bad).is_err());
    let bad = change(&directory, |p| {
        let witness = p.directories[0].witnesses[0].clone();
        p.directories[0].witnesses.push(witness);
    });
    assert!(open(&bad).is_err());
    let fact = directory.directories.get_mut(root.path()).unwrap();
    fact.names = None;
    fact.missing = Some(Text::MissingLeaf);
    assert!(encode(&directory).is_err());
}

#[test]
fn known_variant_payloads_directory_shapes_and_component_bytes_are_strict() {
    let (_root, _path, frozen) = one();
    assert!(
        open(&change(&frozen, |p| {
            let SavedText::Regular { stamp, .. } = &mut p.texts[0].fact else {
                unreachable!()
            };
            stamp.mode = 0o040700;
        }))
        .is_err()
    );
    let source = String::from_utf8(encode(&frozen).unwrap()).unwrap();
    assert!(
        open(
            source
                .replace(r#""kind":"regular""#, r#""kind":"missing_leaf""#)
                .as_bytes()
        )
        .is_err()
    );
    assert!(open(&change(&frozen, |p| p.limits.file = 1_048_577)).is_err());
    let root = root();
    fs::create_dir(root.path().join("drop")).unwrap();
    let (_, frozen) = collect(root.path(), |p| {
        p.entries(&root.path().join("drop")).map(|_| ())
    })
    .unwrap();
    for name in [
        Vec::new(),
        b".".to_vec(),
        b"..".to_vec(),
        b"a/b".to_vec(),
        b"a\0b".to_vec(),
    ] {
        assert!(
            open(&change(&frozen, |p| p.directories[0].names = Some(vec![Raw(name)]))).is_err()
        );
    }
    assert!(
        open(&change(&frozen, |p| p.directories[0].names =
            Some(vec![Raw(b"x".to_vec()), Raw(b"x".to_vec())])))
        .is_err()
    );
}

#[test]
fn lexical_and_encoded_bounds_refuse_before_the_json_parser_can_allocate() {
    assert!(lexical_bounds(&vec![b' '; ENCODED + 1]).is_err());
    assert!(lexical_bounds(&[b'1'; JSON_PRIMITIVE + 1]).is_err());
    let mut string = vec![b'"'];
    string.extend(std::iter::repeat_n(b'x', JSON_STRING + 1));
    string.push(b'"');
    assert!(lexical_bounds(&string).is_err());
    let deep = format!(
        "{}0{}",
        "[".repeat(JSON_DEPTH + 1),
        "]".repeat(JSON_DEPTH + 1)
    );
    assert!(lexical_bounds(deep.as_bytes()).is_err());
    let many = format!("[{}0]", "0,".repeat(JSON_TOKENS));
    assert!(lexical_bounds(many.as_bytes()).is_err());
    let (_root, _path, frozen) = one();
    let mut bytes = encode(&frozen).unwrap();
    bytes.extend_from_slice(b" true");
    assert!(open(&bytes).is_err());
    let mut writer = BoundedWriter(vec![b'x'; ENCODED]);
    let capacity = writer.0.capacity();
    assert!(io::Write::write_all(&mut writer, b"x").is_err());
    assert_eq!(writer.0.len(), ENCODED);
    assert_eq!(writer.0.capacity(), capacity);
}

fn evaluator_fixture(head: bool) -> (tempfile::TempDir, BTreeMap<String, String>, Setting) {
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
fn actual_evaluator_replays_closed_after_restart_for_conditional_carried_and_omitted_inputs() {
    for head in [false, true] {
        for omitted in [false, true] {
            for carried in [false, true] {
                let (root, all, setting) = evaluator_fixture(head);
                let base = root.path().to_owned();
                let units = base.join("etc/systemd/system");
                let carried = if carried {
                    all
                } else {
                    std::collections::BTreeMap::new()
                };
                let settings = [setting];
                let initial = if omitted {
                    Vec::new()
                } else {
                    vec![Step {
                        setting: settings[0].clone(),
                        act: Act::Write,
                    }]
                };
                let mut ordinary = initial.clone();
                head_touch::project(&base, &units, &carried, &settings, &mut ordinary).unwrap();
                head_touch::verify(&base, &units, &carried, &ordinary).unwrap();
                let (expected, frozen) = collect(&base, |inputs| {
                    let mut steps = initial.clone();
                    head_touch::project_with(
                        inputs, &base, &units, &carried, &settings, &mut steps,
                    )?;
                    head_touch::verify_with(inputs, &base, &units, &carried, &steps)?;
                    Ok(steps)
                })
                .unwrap();
                assert_eq!(ordinary, expected);
                assert_eq!(
                    frozen
                        .texts
                        .contains_key(&base.join("etc/lampOS/body/lamp.env")),
                    head
                );
                let bytes = encode(&frozen).unwrap();
                let mut saved = open(&bytes).unwrap();
                saved.revalidate(&base).unwrap();
                root.close().unwrap();
                let mut actual = initial;
                head_touch::project_with(
                    &mut saved,
                    &base,
                    &units,
                    &carried,
                    &settings,
                    &mut actual,
                )
                .unwrap();
                head_touch::verify_with(&mut saved, &base, &units, &carried, &actual).unwrap();
                assert_eq!(actual, expected);
                assert_eq!(saved.as_bytes(), bytes);
                assert!(saved.text(&base.join("uncaptured")).is_err());
                assert!(saved.revalidate(&base).is_err());
            }
        }
    }
}

#[test]
fn hard_link_aliases_keep_two_lexical_keys_and_revalidate_full_content() {
    let root = root();
    let base = root.path();
    let first = put(base, "first", b"equal");
    let second = base.join("second");
    fs::hard_link(&first, &second).unwrap();
    let (_, frozen) = collect(base, |p| {
        p.text(&first)?;
        p.text(&second)?;
        p.entries(base)?;
        Ok(())
    })
    .unwrap();
    let bytes = encode(&frozen).unwrap();
    let mut saved = open(&bytes).unwrap();
    assert_eq!(saved.facts.texts.len(), 2);
    assert_ne!(saved.facts.texts[0].path.0, saved.facts.texts[1].path.0);
    let stamps: Vec<_> = saved
        .facts
        .texts
        .iter()
        .map(|r| {
            let SavedText::Regular { stamp, .. } = &r.fact else {
                panic!("regular");
            };
            stamp
        })
        .collect();
    assert_eq!(stamps[0].ino, stamps[1].ino);
    assert_eq!(stamps[0].nlink, 2);
    saved.revalidate(base).unwrap();
    assert_eq!(saved.text(&first).unwrap().as_deref(), Some("equal"));
    assert_eq!(saved.text(&second).unwrap().as_deref(), Some("equal"));
    fs::write(&first, b"other").unwrap();
    assert!(saved.revalidate(base).is_err());
    assert_eq!(saved.as_bytes(), bytes);
    assert_eq!(saved.text(&second).unwrap().as_deref(), Some("equal"));
}

#[test]
fn revalidation_checks_negative_membership_link_and_root_facts_without_mutating_saved_bytes() {
    for mutation in 0..5 {
        let root = root();
        let base = root.path().to_owned();
        let file = put(&base, "value", b"safe");
        put(&base, "other", b"safe");
        let link = base.join("alias");
        symlink("value", &link).unwrap();
        let leaf = base.join("missing");
        let ancestor = base.join("absent/child");
        let directory = base.join("drop");
        fs::create_dir(&directory).unwrap();
        let (_, frozen) = collect(&base, |p| {
            p.text(&link)?;
            p.text(&leaf)?;
            p.text(&ancestor)?;
            p.entries(&directory)?;
            Ok(())
        })
        .unwrap();
        let bytes = encode(&frozen).unwrap();
        let saved = open(&bytes).unwrap();
        saved.revalidate(&base).unwrap();
        match mutation {
            0 => {
                fs::write(&leaf, b"new").unwrap();
            }
            1 => {
                fs::create_dir(base.join("absent")).unwrap();
            }
            2 => {
                fs::remove_file(&link).unwrap();
                symlink("other", &link).unwrap();
            }
            3 => {
                fs::write(directory.join("ignored.txt"), b"new").unwrap();
            }
            4 => {
                fs::rename(&file, base.join("held-old-inode")).unwrap();
                fs::write(&file, b"safe").unwrap();
            }
            _ => unreachable!(),
        }
        assert!(saved.revalidate(&base).is_err());
        assert_eq!(saved.as_bytes(), bytes);
    }
    let outer = root();
    let base = outer.path().join("root");
    fs::create_dir(&base).unwrap();
    let path = put(&base, "value", b"safe");
    let (_, frozen) = collect(&base, |p| p.text(&path)).unwrap();
    let saved = open(&encode(&frozen).unwrap()).unwrap();
    fs::rename(&base, outer.path().join("old")).unwrap();
    fs::create_dir(&base).unwrap();
    put(&base, "value", b"safe");
    assert!(saved.revalidate(&base).is_err());
}

#[test]
fn revalidation_compares_membership_as_a_set_and_replay_keeps_encoded_order() {
    let root = root();
    let directory = root.path().join("drop");
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("one"), b"").unwrap();
    fs::write(directory.join("two"), b"").unwrap();
    let (_, frozen) = collect(root.path(), |p| p.entries(&directory).map(|_| ())).unwrap();
    // A controlled encoded order is a replay property, not a current readdir-order promise.
    let bytes = change(&frozen, |p| {
        p.directories[0].names.as_mut().unwrap().reverse()
    });
    let mut saved = open(&bytes).unwrap();
    saved.revalidate(root.path()).unwrap();
    let expected = saved.facts.directories[0]
        .names
        .as_ref()
        .unwrap()
        .iter()
        .map(|r| directory.join(OsStr::from_bytes(&r.0)))
        .collect::<Vec<_>>();
    assert_eq!(
        saved
            .entries(&directory)
            .unwrap()
            .unwrap()
            .map(|x| x.unwrap())
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(saved.as_bytes(), bytes);
}

fn missing_wire(frozen: &FrozenInputs, count: usize, witnesses: usize) -> Portable {
    let mut p = Portable::of(frozen).unwrap();
    p.texts.clear();
    p.directories.clear();
    for n in 0..count {
        p.texts.push(TextRow {
            path: Raw(frozen
                .root_path
                .join(format!("v{n:03}"))
                .as_os_str()
                .as_bytes()
                .to_vec()),
            witnesses: vec![SavedWitness::Directory(p.root_stamp.clone()); witnesses],
            fact: SavedText::MissingLeaf,
        });
    }
    p
}
fn wire_bytes(p: &Portable) -> Vec<u8> {
    serde_json::to_vec(p).unwrap()
}

#[test]
fn combined_fact_and_witness_aggregate_limits_are_enforced_across_rows() {
    let (_root, _path, frozen) = one();
    let p = missing_wire(&frozen, 256, 1);
    assert!(open(&wire_bytes(&p)).is_ok());
    let p = missing_wire(&frozen, 257, 1);
    assert!(open(&wire_bytes(&p)).is_err());
    let mut p = missing_wire(&frozen, 128, 64);
    assert_eq!(p.texts.len() * p.texts[0].witnesses.len(), WITNESSES);
    assert!(open(&wire_bytes(&p)).is_ok());
    p.texts.push(TextRow {
        path: Raw(frozen
            .root_path
            .join("v128")
            .as_os_str()
            .as_bytes()
            .to_vec()),
        witnesses: vec![SavedWitness::Directory(p.root_stamp.clone()); 64],
        fact: SavedText::MissingLeaf,
    });
    assert!(open(&wire_bytes(&p)).is_err());
    let mut p = missing_wire(&frozen, 128, 1);
    for n in 0..129 {
        p.directories.push(DirectoryRow {
            path: Raw(frozen
                .root_path
                .join(format!("d{n:03}"))
                .as_os_str()
                .as_bytes()
                .to_vec()),
            witnesses: vec![SavedWitness::Directory(p.root_stamp.clone())],
            names: Some(vec![]),
            missing: None,
        });
    }
    assert!(open(&wire_bytes(&p)).is_err());
    p.directories.pop();
    assert!(open(&wire_bytes(&p)).is_ok());
}

#[test]
fn aggregate_names_raw_link_bytes_and_regular_text_refuse_below_the_encoded_cap() {
    let (_root, _path, frozen) = one();
    let mut p = missing_wire(&frozen, 0, 1);
    for n in 0..65 {
        p.directories.push(DirectoryRow {
            path: Raw(frozen
                .root_path
                .join(format!("d{n:03}"))
                .as_os_str()
                .as_bytes()
                .to_vec()),
            witnesses: vec![SavedWitness::Directory(p.root_stamp.clone())],
            names: Some(
                (0..128)
                    .map(|k| Raw(format!("{k:02x}").into_bytes()))
                    .collect(),
            ),
            missing: None,
        });
    }
    let last = p.directories.pop().unwrap();
    assert!(open(&wire_bytes(&p)).is_ok());
    p.directories.push(last);
    let bytes = wire_bytes(&p);
    assert!(bytes.len() < ENCODED);
    assert!(open(&bytes).is_err());
    p.directories.pop();
    for directory in &mut p.directories {
        directory.names = Some(
            (0..128)
                .map(|n| Raw(format!("{n:08x}").into_bytes()))
                .collect(),
        );
    }
    assert_eq!(
        p.directories
            .iter()
            .map(|d| d
                .names
                .as_ref()
                .unwrap()
                .iter()
                .map(|n| n.0.len())
                .sum::<usize>())
            .sum::<usize>(),
        64 * 1024
    );
    assert!(open(&wire_bytes(&p)).is_ok());
    p.directories[0].names.as_mut().unwrap()[0].0.push(b'x');
    // Counts, component shapes and per-name limits remain valid; only total name bytes grow.
    assert_eq!(
        p.directories
            .iter()
            .map(|d| d.names.as_ref().unwrap().len())
            .sum::<usize>(),
        8192
    );
    assert!(open(&wire_bytes(&p)).is_err());
    let mut p = missing_wire(&frozen, 240, 1);
    let mut link = p.root_stamp.clone();
    link.mode = 0o120777;
    for row in &mut p.texts {
        for _ in 0..2 {
            row.witnesses.push(SavedWitness::Link {
                stamp: link.clone(),
                target: Raw(vec![b'x'; 4096]),
            });
        }
    }
    assert!(open(&wire_bytes(&p)).is_ok());
    for n in 240..256 {
        p.texts.push(TextRow {
            path: Raw(frozen
                .root_path
                .join(format!("v{n:03}"))
                .as_os_str()
                .as_bytes()
                .to_vec()),
            fact: SavedText::MissingLeaf,
            witnesses: vec![
                SavedWitness::Directory(p.root_stamp.clone()),
                SavedWitness::Link {
                    stamp: link.clone(),
                    target: Raw(vec![b'x'; 4096]),
                },
                SavedWitness::Link {
                    stamp: link.clone(),
                    target: Raw(vec![b'x'; 4096]),
                },
            ],
        });
    }
    let bytes = wire_bytes(&p);
    assert!(bytes.len() < ENCODED);
    assert!(open(&bytes).is_err());
    let mut p = missing_wire(&frozen, 5, 1);
    let Text::Regular { identity, .. } = &frozen.texts.values().next().unwrap().text else {
        unreachable!()
    };
    let mut stamp = Stamp::of(*identity).unwrap();
    stamp.size = 1024 * 1024;
    for row in &mut p.texts {
        row.fact = SavedText::Regular {
            text: "x".repeat(1024 * 1024),
            stamp: stamp.clone(),
        };
    }
    let last = p.texts.pop().unwrap();
    assert!(open(&wire_bytes(&p)).is_ok());
    p.texts.push(last);
    let bytes = wire_bytes(&p);
    assert!(bytes.len() < ENCODED);
    assert!(open(&bytes).is_err());
}

#[test]
fn encoder_preflight_refuses_combined_fact_and_text_budgets_without_changing_inputs() {
    let (root, _path, mut frozen) = one();
    frozen.texts.clear();
    for n in 0..256 {
        frozen.texts.insert(
            root.path().join(format!("v{n:03}")),
            TextFact {
                witnesses: vec![Witness::Directory(frozen.root_identity)],
                text: Text::MissingLeaf,
            },
        );
    }
    assert!(check_frozen(&frozen).is_ok());
    frozen.directories.insert(
        root.path().join("d"),
        DirectoryFact {
            witnesses: vec![Witness::Directory(frozen.root_identity)],
            names: Some(vec![]),
            missing: None,
        },
    );
    assert!(check_frozen(&frozen).is_err());
    assert!(encode(&frozen).is_err());
    assert_eq!(frozen.texts.len(), 256);
    assert_eq!(frozen.directories.len(), 1);
    let (_root, _path, mut frozen) = one();
    let Text::Regular { identity, .. } = &frozen.texts.values().next().unwrap().text else {
        unreachable!()
    };
    let mut identity = *identity;
    identity.0.st_size = 1024 * 1024;
    frozen.texts.clear();
    for n in 0..5 {
        frozen.texts.insert(
            frozen.root_path.join(format!("v{n:03}")),
            TextFact {
                witnesses: vec![Witness::Directory(frozen.root_identity)],
                text: Text::Regular {
                    text: "x".repeat(1024 * 1024),
                    identity,
                },
            },
        );
    }
    assert!(encode(&frozen).is_err());
    assert_eq!(frozen.texts.len(), 5);
}

#[test]
fn lookup_parse_and_debug_diagnostics_redact_paths_contents_and_unknown_semantics() {
    let root = root();
    let path = put(root.path(), "secret-leaf-marker", b"secret-content-marker");
    let (_, frozen) = collect(root.path(), |p| p.text(&path)).unwrap();
    let bytes = encode(&frozen).unwrap();
    let mut saved = open(&bytes).unwrap();
    let debug = format!("{saved:?}");
    assert!(!debug.contains("secret") && !debug.contains(&root.path().display().to_string()));
    for error in [
        saved
            .text(&root.path().join("uncaptured-secret-marker"))
            .unwrap_err(),
        saved.entries(&path).err().unwrap(),
    ] {
        assert_eq!(error, REFUSED);
    }
    let mut wire = Portable::of(&frozen).unwrap();
    wire.schema = 2;
    assert_eq!(open(&wire_bytes(&wire)).err().unwrap(), REFUSED);
    let mut wrong = bytes.clone();
    wrong.extend_from_slice(b"secret-invalid-marker");
    assert_eq!(open(&wrong).err().unwrap(), REFUSED);
    assert_eq!(
        decode(&bytes, &[0; 32], bytes.len().try_into().unwrap())
            .err()
            .unwrap(),
        REFUSED
    );
}
