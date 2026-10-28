use super::super::saved as wire;
use super::*;

#[derive(Clone)]
struct Frame {
    tag: u32,
    bytes: Vec<u8>,
}

// Independent test framing only: no production parser/encoder helper is used.
fn frames(bytes: &[u8]) -> Vec<Frame> {
    assert_eq!(&bytes[..8], b"LAMPPLAN");
    assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 1);
    assert_eq!(u32::from_le_bytes(bytes[12..16].try_into().unwrap()), 1);
    let count = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
    let mut at = 20;
    let mut out = Vec::new();
    for _ in 0..count {
        let tag = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let length = usize::try_from(u64::from_le_bytes(
            bytes[at + 4..at + 12].try_into().unwrap(),
        ))
        .unwrap();
        at += 12;
        out.push(Frame {
            tag,
            bytes: bytes[at..at + length].to_vec(),
        });
        at += length;
    }
    assert_eq!(at, bytes.len());
    out
}
fn packed(frames: &[Frame]) -> Vec<u8> {
    let mut bytes = b"LAMPPLAN".to_vec();
    for word in [1, 1, u32::try_from(frames.len()).unwrap()] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    for frame in frames {
        bytes.extend_from_slice(&frame.tag.to_le_bytes());
        bytes.extend_from_slice(&u64::try_from(frame.bytes.len()).unwrap().to_le_bytes());
        bytes.extend_from_slice(&frame.bytes);
    }
    bytes
}
fn identity_word(bytes: &[u8]) -> Vec<u8> {
    let mut word = sha(bytes).to_vec();
    word.extend_from_slice(&u64::try_from(bytes.len()).unwrap().to_le_bytes());
    word
}
fn rebind_blob(rows: &mut [Frame], tag: usize, replacement: &[u8], occurrences: usize) {
    let before = identity_word(&rows[tag - 1].bytes);
    let after = identity_word(replacement);
    let offsets: Vec<_> = rows[0]
        .bytes
        .windows(before.len())
        .enumerate()
        .filter_map(|(i, w)| (w == before).then_some(i))
        .collect();
    assert_eq!(offsets.len(), occurrences);
    for at in offsets {
        rows[0].bytes[at..at + after.len()].copy_from_slice(&after);
    }
    rows[tag - 1].bytes = replacement.to_vec();
}
fn reopened<'a>(
    f: &Fixture,
    expected: &'a ExpectedOrdinary<'a>,
    bytes: Vec<u8>,
) -> Result<wire::CheckedOrdinaryPlan<'a>, Refused> {
    let identity = Artifact::of(&bytes);
    wire::reopen(expected, &identity, &f.public, &f.writer, bytes)
}
fn refuse_saved(result: Result<wire::CheckedOrdinaryPlan<'_>, Refused>) {
    let error = match result {
        Ok(_) => panic!("saved preparation unexpectedly succeeded"),
        Err(error) => error,
    };
    assert_eq!(error.to_string(), "ordinary plan preparation refused");
    assert_eq!(format!("{error:?}"), "ordinary plan preparation refused");
}
fn assert_rows_equal(a: &PreparedOrdinary<'_>, b: &PreparedOrdinary<'_>) {
    assert_eq!(a.manifest, b.manifest);
    assert_eq!(a.signed_bytes, b.signed_bytes);
    assert_eq!(a.projected_order, b.projected_order);
    assert_eq!(a.has_content_changes, b.has_content_changes);
    assert_eq!(a.rows.len(), b.rows.len());
    for (a, b) in a.rows.iter().zip(&b.rows) {
        assert_eq!(a.signed_index, b.signed_index);
        assert_eq!(a.initial, b.initial);
        assert_eq!(a.effective, b.effective);
    }
}

#[test]
fn closed_saved_replay_preserves_signed_source_future_inputs_and_actual_pins_order() {
    for head_on in [false, true] {
        let pins = Setting::new(
            PINS,
            &format!(
                r#"{{"head_pads":[{{"name":"head-1","line":41,"active_low":true,"pull":"up"}}],"legacy_head":{head_on}}}"#
            ),
        );
        let old = pins.text.as_ref().unwrap().as_bytes().to_vec();
        let records = BTreeMap::from([(PINS.into(), pins.sha256.clone().unwrap())]);
        let mut f = Fixture::new(
            vec![pins, Setting::new(A, MARKER)],
            &[(PINS, &old)],
            records,
            BTreeMap::new(),
            Some(head_on),
        );
        f.raw_change(|value| value["future"] = json!({"marker": MARKER}));
        f.head_change(|value| value["future"] = json!([MARKER, {"x": 3}]));
        f.raw.extend_from_slice(b"\n \t");
        f.raw_artifact = Artifact::of(&f.raw);
        f.head.extend_from_slice(b"\n \t");
        f.head_artifact = Artifact::of(&f.head);
        let _ = f.bundle();
        let _ = head_decode(&f.head);
        f.close();
        let records = f.selected();
        let expected = f.expected(&records);
        let prepared = f.run(&expected).unwrap();
        assert_eq!(prepared.rows[0].initial, None);
        assert_eq!(prepared.projected_order, [1, 0]);
        let saved = wire::save(&prepared).unwrap();
        assert_eq!(saved.artifact(), &Artifact::of(saved.as_bytes()));
        let parts = frames(saved.as_bytes());
        assert_eq!(
            parts.iter().map(|f| f.tag).collect::<Vec<_>>(),
            [1, 2, 3, 4, 5, 6]
        );
        assert_eq!(parts[1].bytes, f.envelope);
        assert_eq!(parts[2].bytes, f.raw);
        assert_eq!(parts[3].bytes, f.head);
        let expected_order: Vec<_> = [2_u32, 1, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        assert_eq!(parts[5].bytes, expected_order);
        let checked = wire::reopen(
            &expected,
            &extra_metadata(saved.artifact()),
            &f.public,
            &f.writer,
            saved.as_bytes().to_vec(),
        )
        .unwrap();
        assert_eq!(checked.as_bytes(), saved.as_bytes());
        assert_eq!(checked.artifact(), saved.artifact());
        assert_rows_equal(checked.prepared(), &prepared);
        assert_eq!(checked.prepared().preimages.encoded(), f.raw);
        assert_eq!(checked.prepared().head_inputs.as_bytes(), f.head);
        assert_eq!(
            checked.prepared().manifest.settings[0],
            f.manifest.settings[0]
        );
        assert_ne!(
            checked.prepared().rows[0]
                .effective
                .as_ref()
                .unwrap()
                .setting,
            f.manifest.settings[0]
        );
        for debug in [format!("{saved:?}"), format!("{checked:?}")] {
            for value in [MARKER, PINS, f.root.to_str().unwrap(), &f.transaction] {
                assert!(!debug.contains(value));
            }
        }
    }
}

#[test]
fn complete_none_keep_record_rows_survive_saved_reopen_without_content_qualification() {
    let paths = [
        A,
        B,
        "/etc/lampOS/expression/c",
        "/etc/lampOS/expression/d",
        "/etc/lampOS/expression/e",
    ];
    let settings = vec![
        Setting::new(A, "new"),
        removed(B),
        Setting::new(paths[2], "same"),
        Setting::new(paths[3], "record"),
        Setting::new(paths[4], "removed here"),
    ];
    let records = BTreeMap::from([
        (paths[2].into(), manifest::sha256_hex(b"same")),
        (paths[4].into(), crate::install::SET_ASIDE.into()),
    ]);
    let mut f = Fixture::new(
        settings,
        &[
            (A, &[0, 255, 1]),
            (paths[2], b"same"),
            (paths[3], b"record"),
        ],
        records,
        BTreeMap::new(),
        None,
    );
    f.close();
    let records = f.selected();
    let expected = f.expected(&records);
    let prepared = f.run(&expected).unwrap();
    let saved = wire::save(&prepared).unwrap();
    let checked = reopened(&f, &expected, saved.as_bytes().to_vec()).unwrap();
    let actual = checked.prepared();
    assert_rows_equal(actual, &prepared);
    assert_eq!(actual.rows.len(), 5);
    assert!(matches!(actual.rows[0].initial, Some(Act::Keep(_))));
    assert!(actual.rows[1].initial.is_none() && actual.rows[2].initial.is_none());
    assert_eq!(actual.rows[3].initial, Some(Act::Record));
    assert!(matches!(actual.rows[4].initial, Some(Act::Keep(_))));
    assert_eq!(actual.projected_order, [0, 3, 4]);
    assert!(!actual.has_content_changes);
    assert_eq!(actual.preimages.encoded(), f.raw);
    // A complete all-None plan still has the mandatory four-byte zero-order
    // table; absence of effects is represented rather than dropped.
    let mut f = Fixture::new(
        vec![Setting::new(A, "same"), removed(B)],
        &[(A, b"same")],
        BTreeMap::from([(A.into(), manifest::sha256_hex(b"same"))]),
        BTreeMap::new(),
        None,
    );
    f.close();
    let records = f.selected();
    let expected = f.expected(&records);
    let prepared = f.run(&expected).unwrap();
    assert!(
        prepared
            .rows
            .iter()
            .all(|row| row.initial.is_none() && row.effective.is_none())
    );
    assert!(!prepared.has_content_changes);
    let saved = wire::save(&prepared).unwrap();
    assert_eq!(frames(saved.as_bytes())[5].bytes, 0_u32.to_le_bytes());
    let checked = reopened(&f, &expected, saved.as_bytes().to_vec()).unwrap();
    assert_rows_equal(checked.prepared(), &prepared);
    assert!(checked.prepared().projected_order.is_empty());
    // A real owned removal retains None mode/text and is an actual content
    // change, unlike the absent removal above.
    let mut f = Fixture::new(
        vec![removed(A)],
        &[(A, b"owned")],
        BTreeMap::from([(A.into(), manifest::sha256_hex(b"owned"))]),
        BTreeMap::new(),
        None,
    );
    f.close();
    let records = f.selected();
    let expected = f.expected(&records);
    let prepared = f.run(&expected).unwrap();
    let saved = wire::save(&prepared).unwrap();
    let checked = reopened(&f, &expected, saved.as_bytes().to_vec()).unwrap();
    assert_rows_equal(checked.prepared(), &prepared);
    assert_eq!(checked.prepared().rows[0].initial, Some(Act::Remove));
    assert!(checked.prepared().has_content_changes);
    let effective = checked.prepared().rows[0].effective.as_ref().unwrap();
    assert!(effective.setting.removed);
    assert!(effective.setting.mode.is_none() && effective.setting.text.is_none());
}

#[test]
fn rebound_plan_identity_cannot_substitute_protected_context_writer_or_derived_tables() {
    let mut f = Fixture::ordinary();
    f.close();
    let records = f.selected();
    let expected = f.expected(&records);
    let prepared = f.run(&expected).unwrap();
    let saved = wire::save(&prepared).unwrap();
    let original = frames(saved.as_bytes());
    // Locate mode in this known one-row Write fixture using the documented
    // byte-length prefixes, not the production rows encoder.
    let data = &original[4].bytes;
    assert_eq!(data[8], 1);
    assert_eq!(data[9], 1);
    let mut at = 10;
    let mut payloads = Vec::new();
    for field in 0..3 {
        if field != 0 {
            assert_eq!(data[at], 1);
            at += 1;
        }
        let length =
            usize::try_from(u64::from_le_bytes(data[at..at + 8].try_into().unwrap())).unwrap();
        payloads.push(at + 8..at + 8 + length);
        at += 8 + length;
    }
    assert_eq!(&data[payloads[0].clone()], A.as_bytes());
    assert_eq!(&data[payloads[1].clone()], MARKER.as_bytes());
    assert_eq!(A.len(), B.len());
    let mode = at;
    assert_eq!(data[mode], 1);
    let mode_end = mode + 1 + 8 + 4;
    assert_eq!(&data[mode + 9..mode_end], b"0644");
    assert_eq!(data[data.len() - 2], 1); // final effective Write, before content flag
    for which in 0..12 {
        let mut rows = original.clone();
        match which {
            0 => rows[0].bytes[0] ^= 1, // protected context derivation, frame remains v1
            1 => rows[4].bytes[4..8].copy_from_slice(&1_u32.to_le_bytes()), // signed index
            2 => rows[4].bytes[8] = 3,  // initial Write -> Record
            3 => *rows[4].bytes.last_mut().unwrap() = 0, // content-change summary
            4 => rows[5].bytes[4..8].copy_from_slice(&1_u32.to_le_bytes()), // projected destination
            5 => rows[5].bytes = 0_u32.to_le_bytes().to_vec(), // omitted actual effect
            6 => rows[4].bytes[8] = 255, // unknown action semantics, never default
            7 => {
                rows[4].bytes.splice(mode..mode_end, [0]);
            } // genuine mode None encoding
            8 => rows[4].bytes[mode + 9..mode_end].copy_from_slice(b"0600"),
            9 => rows[4].bytes[payloads[0].clone()].copy_from_slice(B.as_bytes()),
            10 => rows[4].bytes[payloads[1].start] ^= 1,
            _ => {
                let end = rows[4].bytes.len();
                rows[4].bytes[end - 2] = 2;
            }
        }
        refuse_saved(reopened(&f, &expected, packed(&rows)));
    }
    for which in 0..5 {
        let mut changed = f.expected(&records);
        match which {
            0 => changed.product.installed_sequence = None,
            1 => changed.trusted_key_sha256[31] ^= 1,
            2 => changed.product.for_sequence += 1,
            3 => changed.derivation += 1,
            _ => changed.ordinary_enabled = false,
        }
        refuse_saved(reopened(&f, &changed, saved.as_bytes().to_vec()));
    }
    let mut wrong_writer = f.writer.clone();
    wrong_writer[0] ^= 1;
    for bytes in [
        wrong_writer,
        f.writer[..f.writer.len() - 1].to_vec(),
        [f.writer.as_slice(), b"!"].concat(),
    ] {
        refuse_saved(wire::reopen(
            &expected,
            saved.artifact(),
            &f.public,
            &bytes,
            saved.as_bytes().to_vec(),
        ));
    }
    let mut wrong_size = saved.artifact().clone();
    wrong_size.size += 1;
    refuse_saved(wire::reopen(
        &expected,
        &wrong_size,
        &f.public,
        &f.writer,
        saved.as_bytes().to_vec(),
    ));
    let other = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    let other = Ed25519KeyPair::from_pkcs8(other.as_ref()).unwrap();
    let wrong_key: [u8; 32] = other.public_key().as_ref().try_into().unwrap();
    refuse_saved(wire::reopen(
        &expected,
        saved.artifact(),
        &wrong_key,
        &f.writer,
        saved.as_bytes().to_vec(),
    ));
}

#[test]
fn rebound_outer_and_envelope_context_still_require_the_actual_signature() {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let mut f = Fixture::ordinary();
    let mut parts = {
        let records = f.selected();
        let expected = f.expected(&records);
        let prepared = f.run(&expected).unwrap();
        let saved = wire::save(&prepared).unwrap();
        assert!(reopened(&f, &expected, saved.as_bytes().to_vec()).is_ok());
        frames(saved.as_bytes())
    };
    let mut envelope: Value = serde_json::from_slice(&f.envelope).unwrap();
    let signed = envelope["manifest"].clone();
    let fingerprint = envelope["key"].clone();
    let mut signature = STANDARD
        .decode(envelope["signature"].as_str().unwrap())
        .unwrap();
    signature[31] ^= 1;
    envelope["signature"] = json!(STANDARD.encode(signature));
    assert_eq!(envelope["manifest"], signed);
    assert_eq!(envelope["key"], fingerprint);
    f.envelope = serde_json::to_vec(&envelope).unwrap();
    f.envelope_artifact = Artifact::of(&f.envelope);
    f.bind_refresh_envelope();
    rebind_blob(&mut parts, 2, &f.envelope, 2);
    assert!(manifest::open_as(&f.envelope, &f.public, manifest::UPDATER).is_err());
    let _ = f.bundle();
    let _ = head_decode(&f.head);
    f.close();
    let records = f.selected();
    let expected = f.expected(&records);
    refuse_saved(reopened(&f, &expected, packed(&parts)));
}

#[test]
fn independently_decodable_rebound_nested_inputs_cannot_replace_records_or_closed_dependencies() {
    let mut f = Fixture::ordinary();
    let mut parts = {
        let records = f.selected();
        let expected = f.expected(&records);
        let prepared = f.run(&expected).unwrap();
        frames(wire::save(&prepared).unwrap().as_bytes())
    };
    f.raw_change(|value| {
        value["entries"][0]["records"][0] =
            json!(manifest::sha256_hex(b"substituted independent record"))
    });
    rebind_blob(&mut parts, 3, &f.raw, 1);
    let _ = f.bundle();
    let _ = head_decode(&f.head);
    f.close();
    let records = f.selected();
    let expected = f.expected(&records);
    assert_eq!(records[0].recorded, None);
    refuse_saved(reopened(&f, &expected, packed(&parts)));

    let pins = Setting::new(
        PINS,
        r#"{"head_pads":[{"name":"head-1","line":41,"active_low":true,"pull":"up"}],"legacy_head":true}"#,
    );
    let old = pins.text.as_ref().unwrap().as_bytes().to_vec();
    let mut f = Fixture::new(
        vec![Setting::new(A, MARKER), pins],
        &[(PINS, &old)],
        BTreeMap::new(),
        BTreeMap::new(),
        Some(true),
    );
    let mut parts = {
        let records = f.selected();
        let expected = f.expected(&records);
        let prepared = f.run(&expected).unwrap();
        let saved = wire::save(&prepared).unwrap();
        assert!(reopened(&f, &expected, saved.as_bytes().to_vec()).is_ok());
        frames(saved.as_bytes())
    };
    use base64::{Engine, engine::general_purpose::STANDARD};
    let wanted = STANDARD.encode(
        f.root
            .join("etc/lampOS/body/lamp.env")
            .as_os_str()
            .as_encoded_bytes(),
    );
    f.head_change(|value| {
        let rows = value["texts"].as_array_mut().unwrap();
        let before = rows.len();
        rows.retain(|row| row["path"].as_str() != Some(wanted.as_str()));
        assert_eq!(rows.len() + 1, before);
    });
    rebind_blob(&mut parts, 4, &f.head, 1);
    let _ = f.bundle();
    let _ = head_decode(&f.head);
    f.close();
    let records = f.selected();
    let expected = f.expected(&records);
    refuse_saved(reopened(&f, &expected, packed(&parts)));
}

#[test]
fn optional_binary_metadata_retains_exact_bytes_at_count_and_aggregate_boundaries() {
    let mut f = Fixture::ordinary();
    f.close();
    let records = f.selected();
    let expected = f.expected(&records);
    let prepared = f.run(&expected).unwrap();
    let saved = wire::save(&prepared).unwrap();
    let base = frames(saved.as_bytes());
    let mut rows = base.clone();
    let mut metadata = vec![0, 255, 0];
    metadata.extend_from_slice(MARKER.as_bytes());
    rows.push(Frame {
        tag: 0x8000_0001,
        bytes: metadata,
    });
    let bytes = packed(&rows);
    let checked = reopened(&f, &expected, bytes.clone()).unwrap();
    assert_eq!(checked.as_bytes(), bytes);
    assert_eq!(checked.artifact(), &Artifact::of(&bytes));
    assert_rows_equal(checked.prepared(), &prepared);
    assert!(!format!("{checked:?}").contains(MARKER));
    rows = base.clone();
    rows.push(Frame {
        tag: u32::MAX,
        bytes: Vec::new(),
    });
    rows.push(Frame {
        tag: 0x8000_0000,
        bytes: vec![0, 255],
    });
    let bytes = packed(&rows);
    assert_eq!(
        reopened(&f, &expected, bytes.clone()).unwrap().as_bytes(),
        bytes
    );
    for tag in [0x8000_0000, u32::MAX] {
        let mut rows = base.clone();
        rows.push(Frame {
            tag,
            bytes: Vec::new(),
        });
        rows.push(Frame {
            tag,
            bytes: Vec::new(),
        });
        refuse_saved(reopened(&f, &expected, packed(&rows)));
    }
    rows = base.clone();
    rows.push(Frame {
        tag: 0x8000_0000,
        bytes: vec![17; 64 * 1024],
    });
    assert!(reopened(&f, &expected, packed(&rows)).is_ok());
    rows.last_mut().unwrap().bytes.push(17);
    refuse_saved(reopened(&f, &expected, packed(&rows)));
    rows = base.clone();
    for n in 0..16 {
        rows.push(Frame {
            tag: 0x8000_0000 + n,
            bytes: Vec::new(),
        });
    }
    assert!(reopened(&f, &expected, packed(&rows)).is_ok());
    rows.push(Frame {
        tag: 0x8000_0010,
        bytes: Vec::new(),
    });
    refuse_saved(reopened(&f, &expected, packed(&rows)));
    for tag in [1, 7, 0x7fff_ffff, 0x8000_0001] {
        let mut rows = base.clone();
        rows.push(Frame {
            tag: 0x8000_0001,
            bytes: vec![1],
        });
        rows.push(Frame {
            tag,
            bytes: vec![2],
        });
        refuse_saved(reopened(&f, &expected, packed(&rows)));
    }
}

#[test]
fn every_truncated_prefix_and_bounded_framing_corruption_refuses_with_rebound_identity() {
    let mut f = Fixture::ordinary();
    f.close();
    let records = f.selected();
    let expected = f.expected(&records);
    let prepared = f.run(&expected).unwrap();
    let saved = wire::save(&prepared).unwrap();
    assert!(reopened(&f, &expected, saved.as_bytes().to_vec()).is_ok());
    for length in 0..saved.as_bytes().len() {
        refuse_saved(reopened(&f, &expected, saved.as_bytes()[..length].to_vec()));
    }
    for (offset, word) in [(8, 2), (12, 2), (16, 0), (16, 5), (16, 23), (16, u32::MAX)] {
        let mut bytes = saved.as_bytes().to_vec();
        bytes[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
        refuse_saved(reopened(&f, &expected, bytes));
    }
    let mut bytes = saved.as_bytes().to_vec();
    bytes[0] ^= 1;
    refuse_saved(reopened(&f, &expected, bytes));
    let mut bytes = saved.as_bytes().to_vec();
    bytes.extend_from_slice(b"trailing unknown data");
    refuse_saved(reopened(&f, &expected, bytes));
    let mut bytes = saved.as_bytes().to_vec();
    bytes[24..32].copy_from_slice(&u64::MAX.to_le_bytes());
    refuse_saved(reopened(&f, &expected, bytes));
    let base = frames(saved.as_bytes());
    for which in 0..6 {
        let mut rows = base.clone();
        match which {
            0 => rows.swap(1, 2),
            1 => rows[1].tag = 1,
            2 => rows[1].tag = 7,
            3 => rows[0].bytes.clear(),
            4 => rows[4].bytes = vec![0; 2 * 1024 * 1024 + 1],
            _ => rows[5].bytes = vec![0; 1025],
        }
        refuse_saved(reopened(&f, &expected, packed(&rows)));
    }
    // Tiny declarations exceed each cap before any nested copy/allocation.
    let caps = [
        128_u64 * 1024,
        512 * 1024,
        16 * 1024 * 1024,
        16 * 1024 * 1024,
        2 * 1024 * 1024,
        1024,
    ];
    for index in 0..6 {
        let mut rows = base.clone();
        rows[index].bytes.clear();
        refuse_saved(reopened(&f, &expected, packed(&rows)));
    }
    let mut offset = 20;
    for (frame, cap) in base.iter().zip(caps) {
        let mut bytes = saved.as_bytes().to_vec();
        bytes[offset + 4..offset + 12].copy_from_slice(&(cap + 1).to_le_bytes());
        refuse_saved(reopened(&f, &expected, bytes));
        offset += 12 + frame.bytes.len();
    }
    let oversized = vec![0; 40 * 1024 * 1024 + 1];
    refuse_saved(reopened(&f, &expected, oversized));
}
