use super::super::{product::OriginalProductBefore, saved as saved_wire};
use super::*;

const C: &str = "/etc/lampOS/expression/later-product-path";
fn raw_source(bytes: Vec<u8>) -> OriginalProductBefore {
    let identity = Artifact::of(&bytes);
    OriginalProductBefore::from_present(bytes, &identity).unwrap()
}
fn source_json(value: Value) -> OriginalProductBefore {
    raw_source(serde_json::to_vec(&value).unwrap())
}
fn base_doc() -> Value {
    json!({"refused":[],"made":[]})
}
fn state_bytes(f: &Fixture) -> Vec<u8> {
    let state = crate::records::State {
        installed: Some(crate::records::Build {
            sequence: 11,
            commit: "d".repeat(40),
        }),
        settings: f.recorded.clone(),
        settings_writing: f.writing.clone(),
        ..Default::default()
    };
    serde_json::to_vec(&state).unwrap()
}
fn product_refusal<T>(result: Result<T, Refused>) {
    let error = match result {
        Ok(_) => panic!("product projection unexpectedly succeeded"),
        Err(error) => error,
    };
    assert_eq!(error.to_string(), "ordinary plan preparation refused");
    assert_eq!(format!("{error:?}"), "ordinary plan preparation refused");
}
fn malformed(bytes: Vec<u8>) {
    let before = bytes.clone();
    let source = raw_source(bytes);
    product_refusal(source.select(12, &[A]));
    assert_eq!(source.as_bytes(), before);
    for value in [MARKER, A] {
        assert!(!format!("{source:?}").contains(value));
    }
}
fn refusal_entry(sequence: u64) -> Value {
    json!({"sequence":sequence,"commit":"old","why":MARKER,"at_unix_ms":0})
}

#[test]
fn actual_original_state_drives_closed_preparation_and_saved_reopen_with_exact_future_bytes() {
    let old = b"owned old bytes";
    let mut f = Fixture::new(
        vec![Setting::new(A, MARKER), Setting::new(B, "new")],
        &[(A, old)],
        BTreeMap::from([(A.into(), manifest::sha256_hex(old))]),
        BTreeMap::new(),
        None,
    );
    let mut doc: Value = serde_json::from_slice(&state_bytes(&f)).unwrap();
    doc["future"] = json!({"value": MARKER,"nested":[false,{"x":1}]});
    let mut bytes = b" \n".to_vec();
    bytes.extend_from_slice(&serde_json::to_vec(&doc).unwrap());
    bytes.extend_from_slice(b"\n \t");
    f.product = Artifact::of(&bytes);
    f.refresh.product_before = f.product.clone();
    let source =
        OriginalProductBefore::from_present(bytes.clone(), &extra_metadata(&f.product)).unwrap();
    assert_eq!(source.as_bytes(), bytes);
    assert_eq!(source.artifact(), &f.product);
    f.close();
    {
        let wrong_order = source.select(12, &[B, A]).unwrap();
        let records = wrong_order.borrowed_records().unwrap();
        assert_eq!(
            records.iter().map(|row| row.path).collect::<Vec<_>>(),
            [B, A]
        );
        let mut expected = f.expected(&records);
        expected.product = wrong_order.selection(&records).unwrap();
        product_refusal(f.run(&expected));
    }
    let selected = source.select(12, &[A, B]).unwrap();
    let records = selected.borrowed_records().unwrap();
    let mut expected = f.expected(&records);
    expected.product = selected.selection(&records).unwrap();
    assert_eq!(expected.product.installed_sequence, Some(11));
    assert!(!expected.product.refused);
    let prepared = f.run(&expected).unwrap();
    assert!(prepared.has_content_changes);
    assert_eq!(prepared.rows.len(), 2);
    let saved = saved_wire::save(&prepared).unwrap();
    let checked = saved_wire::reopen(
        &expected,
        saved.artifact(),
        &f.public,
        &f.writer,
        saved.as_bytes().to_vec(),
    )
    .unwrap();
    assert_eq!(checked.as_bytes(), saved.as_bytes());
    assert_eq!(checked.prepared().projected_order, prepared.projected_order);
    assert_eq!(checked.prepared().preimages.encoded(), f.raw);
    assert_eq!(source.as_bytes(), bytes);
    for debug in [format!("{source:?}"), format!("{selected:?}")] {
        for secret in [MARKER, A, f.root.to_str().unwrap()] {
            assert!(!debug.contains(secret));
        }
    }
}

#[test]
fn later_signed_new_paths_are_selected_from_the_same_full_original_source() {
    let mut f = Fixture::new(
        vec![Setting::new(A, "initial")],
        &[(A, b"old-a"), (C, &[0, 255, 1])],
        BTreeMap::from([(A.into(), manifest::sha256_hex(b"old-a"))]),
        BTreeMap::from([(C.into(), manifest::sha256_hex(&[0, 255, 1]))]),
        None,
    );
    let bytes = state_bytes(&f);
    f.product = Artifact::of(&bytes);
    f.refresh.product_before = f.product.clone();
    let source = OriginalProductBefore::from_present(bytes.clone(), &f.product).unwrap();
    {
        let initial = source.select(12, &[A]).unwrap();
        let records = initial.borrowed_records().unwrap();
        let mut expected = f.expected(&records);
        expected.product = initial.selection(&records).unwrap();
        assert_eq!(expected.product.records.len(), 1);
        assert!(f.run(&expected).is_ok());
    }
    let original_refresh = f.refresh.envelope.clone();
    f.transaction = "e".repeat(64);
    f.manifest.sequence = 13;
    f.manifest.settings = vec![Setting::new(A, "later-a"), Setting::new(C, "later-c")];
    f.resign();
    f.capture();
    f.close();
    assert_eq!(f.refresh.envelope, original_refresh);
    let later = source.select(13, &[A, C]).unwrap();
    let records = later.borrowed_records().unwrap();
    assert_eq!(records[1].recorded, None);
    assert_eq!(records[1].writing, f.writing.get(C).map(String::as_str));
    let mut expected = f.expected(&records);
    expected.sequence = 13;
    expected.product = later.selection(&records).unwrap();
    assert_eq!(expected.product.artifact, source.artifact());
    assert_eq!(expected.product.for_sequence, 13);
    let prepared = f.run(&expected).unwrap();
    assert_eq!(prepared.rows[1].initial, Some(Act::Write));
    let saved = saved_wire::save(&prepared).unwrap();
    assert!(
        saved_wire::reopen(
            &expected,
            saved.artifact(),
            &f.public,
            &f.writer,
            saved.as_bytes().to_vec()
        )
        .is_ok()
    );
    assert_eq!(source.as_bytes(), bytes);
    let mut live: Value = serde_json::from_slice(&bytes).unwrap();
    live["installed"]["sequence"] = json!(13);
    live["settings"][C] = json!(manifest::sha256_hex(b"post-apply bytes"));
    let changed = serde_json::to_vec(&live).unwrap();
    product_refusal(OriginalProductBefore::from_present(
        changed,
        &f.refresh.product_before,
    ));
}

#[test]
fn defaults_some_zero_refusal_and_ignored_field_limits_match_the_projection_profile() {
    for installed in [
        None,
        Some(Value::Null),
        Some(json!({"sequence":0,"commit":""})),
    ] {
        let mut doc = base_doc();
        if let Some(value) = installed.clone() {
            doc["installed"] = value;
        }
        let source = source_json(doc);
        let selected = source.select(12, &[A]).unwrap();
        let records = selected.borrowed_records().unwrap();
        let projection = selected.selection(&records).unwrap();
        assert_eq!(
            projection.installed_sequence,
            if installed.is_some_and(|x| !x.is_null()) {
                Some(0)
            } else {
                None
            }
        );
        assert!(!projection.refused);
        assert_eq!(records[0].recorded, None);
        assert_eq!(records[0].writing, None);
    }
    let mut doc = base_doc();
    doc["refused"] = json!([refusal_entry(11), refusal_entry(12), refusal_entry(13)]);
    let source = source_json(doc);
    let selected = source.select(12, &[A]).unwrap();
    let records = selected.borrowed_records().unwrap();
    assert!(selected.selection(&records).unwrap().refused);
    let source = source_json(
        json!({"made":[],"refused":[],"previous":17,"systemd_owed":"opaque","models":null,"models_before":[true],"schema":999,"op":"opaque future field"}),
    );
    assert!(serde_json::from_slice::<crate::records::State>(source.as_bytes()).is_err());
    // Known unused fields participate in duplicate checks but provide no
    // ownership/sequence authority and are not fully State-validated here.
    assert!(source.select(12, &[A]).is_ok());
    for field in ["refused", "made"] {
        for value in [Value::Null, json!(false), json!({})] {
            let mut doc = base_doc();
            doc[field] = value;
            malformed(serde_json::to_vec(&doc).unwrap());
        }
        let mut doc = base_doc();
        doc.as_object_mut().unwrap().remove(field);
        malformed(serde_json::to_vec(&doc).unwrap());
    }
    for value in [
        json!(0),
        json!("12"),
        json!([]),
        json!({"sequence":1}),
        json!({"commit":"x"}),
        json!({"sequence":-1,"commit":"x"}),
    ] {
        let mut doc = base_doc();
        doc["installed"] = value;
        malformed(serde_json::to_vec(&doc).unwrap());
    }
    for field in ["settings", "settings_writing"] {
        let mut doc = base_doc();
        doc[field] = Value::Null;
        malformed(serde_json::to_vec(&doc).unwrap());
    }
    for field in ["sequence", "commit", "why", "at_unix_ms"] {
        let mut entry = refusal_entry(12);
        entry.as_object_mut().unwrap().remove(field);
        let mut doc = base_doc();
        doc["refused"] = json!([entry]);
        malformed(serde_json::to_vec(&doc).unwrap());
    }
    malformed(
        br#"{"refused":[],"made":[],"installed":{"sequence":18446744073709551616,"commit":"x"}}"#
            .to_vec(),
    );
    malformed(
        br#"{"refused":[{"sequence":12,"commit":"x","why":"x","at_unix_ms":-1}],"made":[]}"#
            .to_vec(),
    );
}

#[test]
fn source_and_borrowed_record_substitution_do_not_create_product_before_authority() {
    let mut f = Fixture::ordinary();
    let original = state_bytes(&f);
    f.product = Artifact::of(&original);
    f.refresh.product_before = f.product.clone();
    let source = OriginalProductBefore::from_present(original.clone(), &f.product).unwrap();
    f.close();
    let selected = source.select(12, &[A]).unwrap();
    let records = selected.borrowed_records().unwrap();
    let mut expected = f.expected(&records);
    expected.product = selected.selection(&records).unwrap();
    assert!(f.run(&expected).is_ok());
    let mut changed: Value = serde_json::from_slice(&original).unwrap();
    changed["future"] = json!(MARKER);
    let changed = serde_json::to_vec(&changed).unwrap();
    product_refusal(OriginalProductBefore::from_present(
        changed.clone(),
        &f.product,
    ));
    // Even if an untrusted caller recomputes that replacement's hash, the
    // existing prepare still compares it to the protected original Binding.
    let substituted = raw_source(changed);
    let candidate = substituted.select(12, &[A]).unwrap();
    let records = candidate.borrowed_records().unwrap();
    let mut expected = f.expected(&records);
    expected.product = candidate.selection(&records).unwrap();
    product_refusal(f.run(&expected));
    let wrong_paths = source.select(12, &[B]).unwrap();
    let records = wrong_paths.borrowed_records().unwrap();
    let mut expected = f.expected(&records);
    expected.product = wrong_paths.selection(&records).unwrap();
    product_refusal(f.run(&expected));
    let wrong_target = source.select(13, &[A]).unwrap();
    let records = wrong_target.borrowed_records().unwrap();
    let mut expected = f.expected(&records);
    expected.product = wrong_target.selection(&records).unwrap();
    product_refusal(f.run(&expected));
    let mut wrong_size = f.product.clone();
    wrong_size.size += 1;
    product_refusal(OriginalProductBefore::from_present(
        original.clone(),
        &wrong_size,
    ));
    let mut wrong_hash = f.product.clone();
    wrong_hash.sha256 = manifest::sha256_hex(b"other");
    product_refusal(OriginalProductBefore::from_present(
        original.clone(),
        &wrong_hash,
    ));
    let digest = manifest::sha256_hex(b"substituted selected value");
    for which in 0..4 {
        let mut records = selected.borrowed_records().unwrap();
        match which {
            0 => records[0].path = B,
            1 => records[0].recorded = Some(&digest),
            2 => records[0].writing = Some(&digest),
            _ => {
                records.clear();
            }
        }
        product_refusal(selected.selection(&records));
    }
    for bytes in [
        Vec::new(),
        b" ".to_vec(),
        b"null".to_vec(),
        b"{}".to_vec(),
        b"not JSON".to_vec(),
    ] {
        let identity = Artifact::of(&bytes);
        if bytes.is_empty() {
            product_refusal(OriginalProductBefore::from_present(bytes, &identity));
        } else {
            let retained = OriginalProductBefore::from_present(bytes.clone(), &identity).unwrap();
            product_refusal(retained.select(12, &[A]));
            assert_eq!(retained.as_bytes(), bytes);
        }
    }
}

#[test]
fn escaped_known_and_selected_duplicates_and_late_failures_never_yield_partial_selection() {
    let digest = manifest::sha256_hex(b"owned");
    for text in [
        r#"{"refused":[],"made":[],"settings":{},"\u0073ettings":{}}"#.to_owned(),
        r#"{"refused":[],"made":[],"previous":null,"\u0070revious":false}"#.to_owned(),
        r#"{"refused":[],"made":[],"installed":{"sequence":1,"\u0073equence":2,"commit":"x"}}"#.to_owned(),
        r#"{"refused":[{"sequence":1,"commit":"x","\u0063ommit":"y","why":"x","at_unix_ms":0}],"made":[]}"#.to_owned(),
        format!(r#"{{"refused":[],"made":[],"settings":{{"{A}":"{digest}","{}":"{digest}"}}}}"#, A.replace('/', "\\/")),
        format!(r#"{{"refused":[],"made":[],"settings_writing":{{"{A}":"{digest}","{}":"{}"}}}}"#, A.replace('/', "\\/"), manifest::sha256_hex(b"other")),
        format!(r#"{{"settings":{{"{A}":"{digest}"}},"refused":[],"made":["ok",false]}}"#),
        format!(r#"{{"settings":{{"{A}":"{digest}"}},"refused":[],"made":[]}} trailing"#),
    ] { malformed(text.into_bytes()); }
    for value in ["NOT-A-DIGEST".to_owned(), "A".repeat(64), "a".repeat(65)] {
        let mut doc = base_doc();
        doc["settings"] = json!({A:value});
        malformed(serde_json::to_vec(&doc).unwrap());
    }
    let mut doc = base_doc();
    doc["settings_writing"] = json!({A:crate::install::SET_ASIDE});
    malformed(serde_json::to_vec(&doc).unwrap());
    let mut doc = base_doc();
    doc["settings"] = json!({C:42});
    malformed(serde_json::to_vec(&doc).unwrap());
    let text = format!(
        r#"{{"refused":[],"made":[],"future":1,"future":2,"settings":{{"{C}":"{digest}","{}":"{}"}}}}"#,
        C.replace('/', "\\/"),
        manifest::sha256_hex(b"other-valid-hash")
    );
    let source = raw_source(text.into_bytes());
    assert!(source.select(12, &[A]).is_ok());
    product_refusal(source.select(12, &[C]));
    let mut bytes = br#"{"refused":[],"made":[],"future":""}"#.to_vec();
    let at = bytes.len() - 3;
    bytes.insert(at, 255);
    malformed(bytes);
}

#[test]
fn selected_marker_writing_hash_order_and_request_limits_are_exact() {
    let digest = manifest::sha256_hex(b"writing");
    let source = source_json(
        json!({"refused":[],"made":[],"settings":{A:crate::install::SET_ASIDE},"settings_writing":{B:digest}}),
    );
    let selected = source.select(12, &[B, A, C]).unwrap();
    let records = selected.borrowed_records().unwrap();
    assert_eq!(
        records.iter().map(|x| x.path).collect::<Vec<_>>(),
        [B, A, C]
    );
    assert_eq!(records[0].recorded, None);
    assert_eq!(records[0].writing, Some(digest.as_str()));
    assert_eq!(records[1].recorded, Some(crate::install::SET_ASIDE));
    assert_eq!(records[2].recorded, None);
    let mut reversed = selected.borrowed_records().unwrap();
    reversed.reverse();
    product_refusal(selected.selection(&reversed));
    for paths in [
        vec![],
        vec![A, A],
        vec![A, "/etc/../secret"],
        vec![A, "relative"],
    ] {
        product_refusal(source.select(12, &paths));
    }
    product_refusal(source.select(0, &[A]));
    let names: Vec<_> = (0..128)
        .map(|n| format!("/etc/lampOS/expression/p{n}"))
        .collect();
    let paths: Vec<_> = names.iter().map(String::as_str).collect();
    let mut doc = base_doc();
    doc["settings"] = json!({names[127].as_str():digest});
    let source = source_json(doc);
    let selected = source.select(12, &paths).unwrap();
    let records = selected.borrowed_records().unwrap();
    assert_eq!(records.len(), 128);
    assert_eq!(records[127].recorded, Some(digest.as_str()));
    let mut too_many = paths.clone();
    too_many.push(C);
    product_refusal(source.select(12, &too_many));
    let prefix = "/etc/lampOS/expression/";
    let long = format!(
        "{prefix}{}/{}/{}",
        "a".repeat(160),
        "b".repeat(160),
        "c".repeat(511 - prefix.len() - 322)
    );
    assert_eq!(long.len(), 511);
    assert!(source.select(12, &[&long]).is_ok());
    product_refusal(source.select(12, &[&(long + "x")]));
    let deep = format!("{prefix}{}leaf", "d/".repeat(60));
    assert_eq!(deep.trim_start_matches('/').split('/').count(), 64);
    assert!(source.select(12, &[&deep]).is_ok());
    product_refusal(source.select(12, &[&(deep + "/x")]));
}

#[test]
fn collection_caps_admit_exact_limits_and_charge_unrelated_entries_too() {
    for scope in 0..3 {
        let mut doc = base_doc();
        if scope == 1 {
            doc["installed"] = json!({"sequence":11,"commit":"x"});
        }
        if scope == 2 {
            doc["refused"] = json!([refusal_entry(11)]);
        }
        let map = match scope {
            0 => doc.as_object_mut().unwrap(),
            1 => doc["installed"].as_object_mut().unwrap(),
            _ => doc["refused"][0].as_object_mut().unwrap(),
        };
        let required = map.len();
        for n in 0..4096 - required {
            map.insert(format!("opaque-{n}"), Value::Null);
        }
        assert_eq!(map.len(), 4096);
        assert!(source_json(doc.clone()).select(12, &[A]).is_ok());
        let map = match scope {
            0 => doc.as_object_mut().unwrap(),
            1 => doc["installed"].as_object_mut().unwrap(),
            _ => doc["refused"][0].as_object_mut().unwrap(),
        };
        map.insert("one-over".into(), Value::Null);
        malformed(serde_json::to_vec(&doc).unwrap());
    }
    for field in ["settings", "settings_writing"] {
        let digest = manifest::sha256_hex(b"owned");
        let mut map = serde_json::Map::new();
        for n in 0..4095 {
            map.insert(format!("unrelated-{n}"), json!(""));
        }
        map.insert(A.into(), json!(digest));
        let mut doc = base_doc();
        doc[field] = Value::Object(map);
        let source = source_json(doc.clone());
        let selected = source.select(12, &[A]).unwrap();
        let records = selected.borrowed_records().unwrap();
        assert_eq!(
            if field == "settings" {
                records[0].recorded
            } else {
                records[0].writing
            },
            Some(digest.as_str())
        );
        doc[field]
            .as_object_mut()
            .unwrap()
            .insert("one-over".into(), json!(""));
        malformed(serde_json::to_vec(&doc).unwrap());
    }
    let mut doc = base_doc();
    doc["made"] = json!(vec![""; 4096]);
    assert!(source_json(doc.clone()).select(12, &[A]).is_ok());
    doc["made"].as_array_mut().unwrap().push(json!(""));
    malformed(serde_json::to_vec(&doc).unwrap());
    let mut doc = base_doc();
    doc["refused"] = Value::Array((0..32).map(refusal_entry).collect());
    let source = source_json(doc.clone());
    let selected = source.select(12, &[A]).unwrap();
    let records = selected.borrowed_records().unwrap();
    assert!(selected.selection(&records).unwrap().refused);
    doc["refused"]
        .as_array_mut()
        .unwrap()
        .push(refusal_entry(33));
    malformed(serde_json::to_vec(&doc).unwrap());
    let mut doc = base_doc();
    doc["made"] = json!(["x".repeat(4096)]);
    assert!(source_json(doc.clone()).select(12, &[A]).is_ok());
    doc["made"] = json!(["x".repeat(4097)]);
    malformed(serde_json::to_vec(&doc).unwrap());
}

#[test]
fn whole_source_lexical_escape_depth_start_and_primitive_bounds_precede_projection() {
    for length in [4096, 4097] {
        let key = "\\u0061".repeat(length);
        for text in [
            format!(r#"{{"refused":[],"made":[],"settings":{{"{key}":""}}}}"#),
            format!(r#"{{"refused":[],"made":[],"{key}":null}}"#),
        ] {
            let source = raw_source(text.into_bytes());
            if length == 4096 {
                assert!(source.select(12, &[A]).is_ok());
            } else {
                product_refusal(source.select(12, &[A]));
            }
        }
    }
    for length in [65536, 65537] {
        let mut doc = base_doc();
        doc["future"] = json!("x".repeat(length));
        let source = source_json(doc);
        if length == 65536 {
            assert!(source.select(12, &[A]).is_ok());
        } else {
            product_refusal(source.select(12, &[A]));
        }
    }
    for extra in [0, 1] {
        let content = "\\u0061".repeat(10922) + &"a".repeat(4 + extra);
        assert_eq!(content.len(), 65536 + extra);
        let text = format!(r#"{{"refused":[],"made":[],"future":"{content}"}}"#);
        let source = raw_source(text.into_bytes());
        if extra == 0 {
            assert!(source.select(12, &[A]).is_ok());
        } else {
            product_refusal(source.select(12, &[A]));
        }
    }
    for arrays in [31, 32] {
        let text = format!(
            r#"{{"refused":[],"made":[],"future":{}0{}}}"#,
            "[".repeat(arrays),
            "]".repeat(arrays)
        );
        assert!(serde_json::from_slice::<crate::records::State>(text.as_bytes()).is_ok());
        let source = raw_source(text.into_bytes());
        if arrays == 31 {
            assert!(source.select(12, &[A]).is_ok());
        } else {
            product_refusal(source.select(12, &[A]));
        }
    }
    for n in [131072 - 7, 131072 - 6] {
        let items = std::iter::repeat_n("0", n).collect::<Vec<_>>().join(",");
        let text = format!(r#"{{"refused":[],"made":[],"future":[{items}]}}"#);
        let source = raw_source(text.into_bytes());
        if n == 131072 - 7 {
            assert!(source.select(12, &[A]).is_ok());
        } else {
            product_refusal(source.select(12, &[A]));
        }
    }
    for length in [128, 129] {
        let number = "0.".to_owned() + &"0".repeat(length - 2);
        let text = format!(r#"{{"refused":[],"made":[],"future":{number}}}"#);
        let source = raw_source(text.into_bytes());
        if length == 128 {
            assert!(source.select(12, &[A]).is_ok());
        } else {
            product_refusal(source.select(12, &[A]));
        }
    }
    let body = br#"{"refused":[],"made":[]}"#;
    let mut exact = vec![b' '; 16 * 1024 * 1024 - body.len()];
    exact.extend_from_slice(body);
    let source = raw_source(exact);
    assert_eq!(source.as_bytes().len(), 16 * 1024 * 1024);
    assert!(source.select(12, &[A]).is_ok());
    let over = vec![b' '; 16 * 1024 * 1024 + 1];
    let identity = Artifact::of(&over);
    product_refusal(OriginalProductBefore::from_present(over, &identity));
    for bytes in [
        b"{]".to_vec(),
        b"{\"refused\":[],\"made\":[]}{}".to_vec(),
        br#"{"refused":[],"made":[],"future":"\q"}"#.to_vec(),
    ] {
        malformed(bytes);
    }
    // IgnoredAny validates escape syntax, not decoded Unicode scalar values.
    // Raw UTF8 is valid here; no decoded future string becomes an authority.
    let opaque = raw_source(br#"{"refused":[],"made":[],"future":"\uD800"}"#.to_vec());
    assert!(serde_json::from_slice::<crate::records::State>(opaque.as_bytes()).is_err());
    assert!(opaque.select(12, &[A]).is_ok());
    assert_eq!(
        opaque.as_bytes(),
        br#"{"refused":[],"made":[],"future":"\uD800"}"#
    );
    malformed(
        br#"{"refused":[],"made":[],"installed":{"sequence":11,"commit":"\uD800"}}"#.to_vec(),
    );
}
