use super::*;
use crate::{
    apply::{self, Act, Step},
    head_touch::{
        self,
        capture::{self, codec},
    },
    manifest::{self, File, ImageFile, Manifest, Setting, Unit},
    writer_preimages::Bundle,
    writer_refresh::{Artifact, Binding},
    writer_settings,
};
use ring::{
    rand::SystemRandom,
    signature::{Ed25519KeyPair, KeyPair},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

const A: &str = "/etc/lampOS/expression/private-plan-a";
const B: &str = "/etc/lampOS/expression/private-plan-b";
const PINS: &str = "/etc/lampOS/controls/controls-pins.json";
const MARKER: &str = "private-plan-value-must-not-escape";

fn sha(bytes: &[u8]) -> [u8; 32] {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .try_into()
        .unwrap()
}
fn put(root: &Path, path: &str, bytes: &[u8]) {
    let at = root.join(path.trim_start_matches('/'));
    fs::create_dir_all(at.parent().unwrap()).unwrap();
    fs::write(at, bytes).unwrap();
}
fn removed(path: &str) -> Setting {
    Setting {
        path: path.into(),
        text: None,
        sha256: None,
        mode: None,
        removed: true,
    }
}
fn head_decode(bytes: &[u8]) -> codec::SavedInputs {
    codec::decode(bytes, &sha(bytes), bytes.len().try_into().unwrap())
        .expect("the existing Head decoder must accept the fixture")
}
fn extra_metadata(artifact: &Artifact) -> Artifact {
    let mut value = serde_json::to_value(artifact).unwrap();
    value["future"] = json!({"marker": MARKER});
    serde_json::from_value(value).unwrap()
}

struct Fixture {
    temporary: Option<tempfile::TempDir>,
    root: PathBuf,
    units: PathBuf,
    key: Ed25519KeyPair,
    public: [u8; 32],
    writer: Vec<u8>,
    manifest: Manifest,
    signed: Vec<u8>,
    envelope: Vec<u8>,
    envelope_artifact: Artifact,
    refresh: Binding,
    transaction: String,
    product: Artifact,
    recorded: BTreeMap<String, String>,
    writing: BTreeMap<String, String>,
    raw: Vec<u8>,
    raw_artifact: Artifact,
    head: Vec<u8>,
    head_artifact: Artifact,
}
impl Fixture {
    fn new(
        settings: Vec<Setting>,
        old: &[(&str, &[u8])],
        recorded: BTreeMap<String, String>,
        writing: BTreeMap<String, String>,
        head_on: Option<bool>,
    ) -> Self {
        let temporary = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let root = temporary.path().to_owned();
        let units = root.join("etc/systemd/system");
        fs::create_dir_all(&units).unwrap();
        if let Some(head) = head_on {
            let unit = |name: &str, args: &str, environment: &str| {
                format!(
                    "[Service]\nExecStart=/opt/lampOS/{name}/current/lamp-{name} {args}\n{environment}"
                )
            };
            for (name, text) in [
                ("controls", unit("controls", &format!("--pins {PINS}"), "")),
                (
                    "voice",
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
                    "expression",
                    unit("expression", if head { "--head-touch" } else { "" }, ""),
                ),
                (
                    "body",
                    unit(
                        "body",
                        "%H $BODY_POSES",
                        "Environment=LAMP_HEAD_TOUCH=on\nEnvironmentFile=/etc/lampOS/body/lamp.env\nEnvironmentFile=-/var/lib/lampos-body/measured.env\n",
                    ),
                ),
            ] {
                put(
                    &root,
                    &format!("etc/systemd/system/lamp-{name}.service"),
                    text.as_bytes(),
                );
            }
            put(
                &root,
                "/etc/lampOS/body/lamp.env",
                b"BODY_POSES=\"1,2,3\"\n",
            );
            fs::create_dir(units.join("service.d")).unwrap();
        }
        for (path, bytes) in old {
            put(&root, path, bytes);
        }
        let document = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        let key = Ed25519KeyPair::from_pkcs8(document.as_ref()).unwrap();
        let public = key.public_key().as_ref().try_into().unwrap();
        let writer = b"private test writer buffer".to_vec();
        let manifest = Manifest {
            schema: manifest::SCHEMA,
            channel: "lamp-voice".into(),
            sequence: 12,
            commit: "a".repeat(40),
            files_url: "https://example.invalid/build/".into(),
            files: vec![File {
                name: "lamp-update".into(),
                sha256: manifest::sha256_hex(&writer),
                size: writer.len().try_into().unwrap(),
            }],
            units: Vec::new(),
            min_updater: Some(manifest::UPDATER),
            models: Vec::new(),
            settings,
            image: Vec::new(),
            reset_plan: None,
        };
        let product = Artifact::of(b"independent protected product-before snapshot");
        let mut fixture = Self {
            temporary: Some(temporary),
            root,
            units,
            key,
            public,
            writer,
            manifest,
            signed: Vec::new(),
            envelope: Vec::new(),
            envelope_artifact: Artifact::of(b"pending"),
            refresh: Binding::new(
                "b".repeat(64),
                12,
                Artifact::of(b"pending"),
                Artifact::of(b"frozen guard"),
                Artifact::of(b"pending"),
                Artifact::of(b"previous writer"),
                product.clone(),
            ),
            transaction: "c".repeat(64),
            product,
            recorded,
            writing,
            raw: Vec::new(),
            raw_artifact: Artifact::of(b"pending"),
            head: Vec::new(),
            head_artifact: Artifact::of(b"pending"),
        };
        fixture.resign();
        fixture.refresh.envelope = fixture.envelope_artifact.clone();
        fixture.refresh.writer = Artifact::of(&fixture.writer);
        fixture.capture();
        fixture
    }
    fn ordinary() -> Self {
        Self::new(
            vec![Setting::new(A, MARKER)],
            &[],
            BTreeMap::new(),
            BTreeMap::new(),
            None,
        )
    }
    fn resign(&mut self) {
        self.signed = serde_json::to_vec(&self.manifest).unwrap();
        self.envelope = manifest::seal_unchecked(&self.signed, &self.key).unwrap();
        self.envelope_artifact = Artifact::of(&self.envelope);
    }
    fn bind_refresh_envelope(&mut self) {
        self.refresh.envelope = self.envelope_artifact.clone();
    }
    fn capture(&mut self) {
        let raw = writer_settings::capture(
            &self.root,
            &self.manifest.settings,
            &self.recorded,
            &self.writing,
        )
        .unwrap();
        self.raw = Bundle::from_capture(&self.transaction, &raw)
            .unwrap()
            .encoded()
            .to_vec();
        self.raw_artifact = Artifact::of(&self.raw);
        let initial = apply::plan(
            &self.root,
            &self.manifest.settings,
            &self.recorded,
            &self.writing,
            &BTreeMap::new(),
        )
        .unwrap();
        let (_, frozen) = capture::collect(&self.root, |inputs| {
            let projected = head_touch::plan_output::project(
                inputs,
                &self.root,
                &self.units,
                &BTreeMap::new(),
                &self.manifest.settings,
                initial,
            )?;
            head_touch::verify_with(
                inputs,
                &self.root,
                &self.units,
                &BTreeMap::new(),
                &projected,
            )?;
            Ok(())
        })
        .unwrap();
        self.head = codec::encode(&frozen).unwrap();
        self.head_artifact = Artifact::of(&self.head);
    }
    fn selected(&self) -> Vec<SelectedRecord<'_>> {
        self.manifest
            .settings
            .iter()
            .map(|setting| SelectedRecord {
                path: &setting.path,
                recorded: self.recorded.get(&setting.path).map(String::as_str),
                writing: self.writing.get(&setting.path).map(String::as_str),
            })
            .collect()
    }
    fn expected<'a>(&'a self, records: &'a [SelectedRecord<'a>]) -> ExpectedOrdinary<'a> {
        ExpectedOrdinary {
            transaction: &self.transaction,
            envelope: &self.envelope_artifact,
            channel: "lamp-voice",
            sequence: 12,
            commit: &self.manifest.commit,
            trusted_key_sha256: sha(&self.public),
            refresh: &self.refresh,
            product: ProductSelection {
                artifact: &self.product,
                for_sequence: 12,
                installed_sequence: Some(11),
                refused: false,
                records,
            },
            preimages: &self.raw_artifact,
            head_inputs: &self.head_artifact,
            root: &self.root,
            installed_units: &self.units,
            ordinary_enabled: true,
            derivation: ORDINARY_DERIVATION,
        }
    }
    fn bundle(&self) -> Bundle {
        Bundle::decode(
            self.raw.clone(),
            &self.transaction,
            &Artifact::of(&self.raw),
        )
        .expect("the existing Bundle decoder must accept the fixture")
    }
    fn run<'a>(&self, expected: &'a ExpectedOrdinary<'a>) -> Result<PreparedOrdinary<'a>, Refused> {
        prepare(
            expected,
            &self.envelope,
            &self.public,
            &self.writer,
            self.bundle(),
            head_decode(&self.head),
        )
    }
    fn close(&mut self) {
        self.temporary.take().unwrap().close().unwrap();
    }
    fn raw_change(&mut self, change: impl FnOnce(&mut Value)) {
        let mut value: Value = serde_json::from_slice(&self.raw).unwrap();
        change(&mut value);
        self.raw = serde_json::to_vec(&value).unwrap();
        self.raw_artifact = Artifact::of(&self.raw);
        let _ = self.bundle();
    }
    fn head_change(&mut self, change: impl FnOnce(&mut Value)) {
        let mut value: Value = serde_json::from_slice(&self.head).unwrap();
        change(&mut value);
        self.head = serde_json::to_vec(&value).unwrap();
        self.head_artifact = Artifact::of(&self.head);
        let _ = head_decode(&self.head);
    }
}
fn refusal(result: Result<PreparedOrdinary<'_>, Refused>) {
    let error = match result {
        Ok(_) => panic!("preparation unexpectedly succeeded"),
        Err(error) => error,
    };
    assert_eq!(error.to_string(), "ordinary plan preparation refused");
    assert_eq!(format!("{error:?}"), "ordinary plan preparation refused");
}

#[test]
fn closed_preparation_retains_exact_signed_and_additive_encoded_bytes() {
    let mut f = Fixture::ordinary();
    f.raw_change(|value| value["future"] = json!({"private": MARKER}));
    f.head_change(|value| value["future"] = json!([MARKER, {"n": 1}]));
    f.envelope_artifact = extra_metadata(&f.envelope_artifact);
    f.product = extra_metadata(&f.product);
    f.close();
    let records = f.selected();
    let expected = f.expected(&records);
    for _ in 0..2 {
        let prepared = f.run(&expected).unwrap();
        assert_eq!(prepared.envelope.as_ref(), f.envelope);
        assert_eq!(prepared.signed_bytes, f.signed);
        assert_eq!(prepared.manifest, f.manifest);
        assert_eq!(prepared.preimages.encoded(), f.raw);
        assert_eq!(prepared.head_inputs.as_bytes(), f.head);
        assert_eq!(prepared.rows.len(), 1);
        assert_eq!(prepared.rows[0].initial, Some(Act::Write));
        assert_eq!(prepared.projected_order, [0]);
        assert!(prepared.has_content_changes);
        let debug = format!("{prepared:?}");
        for secret in [
            MARKER,
            A,
            f.root.to_str().unwrap(),
            &f.transaction,
            &f.manifest.commit,
        ] {
            assert!(!debug.contains(secret));
        }
    }
}

#[test]
fn authentication_and_protected_release_fields_are_independent_checks() {
    let f = Fixture::ordinary();
    let records = f.selected();
    for field in 0..6 {
        let mut expected = f.expected(&records);
        match field {
            0 => expected.trusted_key_sha256[31] ^= 1,
            1 => expected.channel = "other-channel",
            2 => expected.sequence += 1,
            3 => expected.commit = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
            4 => expected.envelope = &f.product,
            _ => expected.product.for_sequence += 1,
        }
        refusal(f.run(&expected));
    }
    let other = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    let other = Ed25519KeyPair::from_pkcs8(other.as_ref()).unwrap();
    let wrong: [u8; 32] = other.public_key().as_ref().try_into().unwrap();
    let mut expected = f.expected(&records);
    // Even a protected identity for this other whole key cannot validate our signature.
    expected.trusted_key_sha256 = sha(&wrong);
    refusal(prepare(
        &expected,
        &f.envelope,
        &wrong,
        &f.writer,
        f.bundle(),
        head_decode(&f.head),
    ));
    for min in [None, Some(manifest::UPDATER + 1)] {
        let mut f = Fixture::ordinary();
        f.manifest.min_updater = min;
        f.resign();
        f.bind_refresh_envelope();
        let records = f.selected();
        let expected = f.expected(&records);
        if min.is_none() {
            // Settings still require their capability even when the field is omitted.
            refusal(f.run(&expected));
        } else {
            assert!(manifest::open_as(&f.envelope, &f.public, manifest::UPDATER).is_err());
            refusal(f.run(&expected));
        }
    }
}

#[test]
fn signed_writer_hash_size_and_each_offered_byte_buffer_are_bound() {
    for change in 0..2 {
        let mut f = Fixture::ordinary();
        if change == 0 {
            f.manifest.files[0].sha256 = manifest::sha256_hex(b"other writer");
        } else {
            f.manifest.files[0].size += 1;
        }
        f.resign();
        f.bind_refresh_envelope();
        assert!(manifest::open_as(&f.envelope, &f.public, manifest::UPDATER).is_ok());
        let records = f.selected();
        refusal(f.run(&f.expected(&records)));
    }
    let f = Fixture::ordinary();
    let records = f.selected();
    let expected = f.expected(&records);
    let mut changed = f.writer.clone();
    changed[0] ^= 1;
    for bytes in [
        changed,
        f.writer[..f.writer.len() - 1].to_vec(),
        [f.writer.as_slice(), b"!"].concat(),
    ] {
        refusal(prepare(
            &expected,
            &f.envelope,
            &f.public,
            &bytes,
            f.bundle(),
            head_decode(&f.head),
        ));
    }
}

#[test]
fn later_same_writer_is_permitted_but_refresh_reuse_and_product_substitution_refuse() {
    let mut f = Fixture::ordinary();
    f.manifest.sequence = 13;
    f.resign();
    f.close();
    let records = f.selected();
    let mut expected = f.expected(&records);
    expected.sequence = 13;
    expected.product.for_sequence = 13;
    assert!(f.run(&expected).unwrap().has_content_changes);
    expected.transaction = &f.refresh.transaction;
    refusal(f.run(&expected));
    let mut f = Fixture::ordinary();
    f.manifest.files_url.push_str("later/");
    f.resign();
    let records = f.selected();
    refusal(f.run(&f.expected(&records)));
    let f = Fixture::ordinary();
    let records = f.selected();
    for which in 0..4 {
        let mut expected = f.expected(&records);
        match which {
            0 => expected.product.installed_sequence = Some(12),
            1 => expected.product.installed_sequence = Some(13),
            2 => expected.product.refused = true,
            _ => expected.product.artifact = &f.raw_artifact,
        }
        refusal(f.run(&expected));
    }
}

#[test]
fn unsupported_effects_refuse_even_when_selected_settings_would_be_noops() {
    for which in 0..9 {
        let mut f = Fixture::new(
            vec![Setting::new(A, "unchanged")],
            &[(A, b"unchanged")],
            BTreeMap::from([(A.into(), manifest::sha256_hex(b"unchanged"))]),
            BTreeMap::new(),
            None,
        );
        match which {
            0 => f.manifest.files.push(File {
                name: "lamp-voice".into(),
                sha256: manifest::sha256_hex(b"voice"),
                size: 5,
            }),
            1 | 2 => f.manifest.units.push(Unit {
                lamp: if which == 1 {
                    "fixture-lamp"
                } else {
                    manifest::PRODUCT_LAMP
                }
                .into(),
                unit: "lamp-controls.service".into(),
                sha256: manifest::sha256_hex(b"unit"),
                size: 4,
            }),
            3 => f.manifest.models.push(manifest::Model {
                path: "/opt/lampOS/lamp-voice/test.bin".into(),
                url: "https://example.invalid/model".into(),
                bytes: 1,
                sha256: manifest::sha256_hex(b"m"),
                archive: None,
            }),
            4 => f.manifest.image.push(ImageFile {
                variant: "standard".into(),
                path: "/etc/asound.conf".into(),
                text: "same".into(),
                sha256: manifest::sha256_hex(b"same"),
                shipped: vec![manifest::sha256_hex(b"same")],
            }),
            5 => f.manifest.reset_plan = Some(Box::new(Setting::new(manifest::RESET_PLAN, "{}"))),
            6 | 7 => {
                let path = if which == 6 {
                    "/etc/tmpfiles.d/lampos.conf"
                } else {
                    "/var/lib/lampOS/skills/store-key.pub"
                };
                f.manifest.settings = vec![Setting::new(path, "unchanged")];
                f.recorded = BTreeMap::from([(path.into(), manifest::sha256_hex(b"unchanged"))]);
                put(&f.root, path, b"unchanged");
                f.capture();
            }
            _ => {
                f.manifest.settings.clear();
                f.recorded.clear();
                f.capture();
            }
        }
        f.resign();
        f.bind_refresh_envelope();
        manifest::open_as(&f.envelope, &f.public, manifest::UPDATER)
            .expect("the signed unsupported-effect fixture must otherwise be readable");
        let records = f.selected();
        refusal(f.run(&f.expected(&records)));
    }
}

#[test]
fn ordered_paths_selected_records_and_protected_artifacts_cannot_be_rebound_by_bundle() {
    for which in 0..4 {
        let mut f = Fixture::new(
            vec![Setting::new(A, "a"), Setting::new(B, "b")],
            &[],
            BTreeMap::new(),
            BTreeMap::new(),
            None,
        );
        f.raw_change(|value| match which {
            0 => value["entries"].as_array_mut().unwrap().reverse(),
            1 => {
                value["entries"].as_array_mut().unwrap().pop();
            }
            2 => {
                let mut row = value["entries"][1].clone();
                row["path"] = json!("/etc/lampOS/expression/extra");
                value["entries"].as_array_mut().unwrap().push(row);
            }
            _ => {
                value["entries"][0]["records"][0] =
                    json!(manifest::sha256_hex(b"substituted record"))
            }
        });
        let records = f.selected();
        refusal(f.run(&f.expected(&records)));
    }
    let f = Fixture::ordinary();
    let records = f.selected();
    for which in 0..3 {
        let mut expected = f.expected(&records);
        match which {
            0 => expected.transaction = &f.refresh.transaction,
            1 => expected.preimages = &f.product,
            _ => expected.head_inputs = &f.product,
        }
        refusal(f.run(&expected));
    }
    let mut wrong_transaction: Value = serde_json::from_slice(&f.raw).unwrap();
    let other_transaction = "d".repeat(64);
    wrong_transaction["transaction"] = json!(other_transaction);
    let wrong_transaction = serde_json::to_vec(&wrong_transaction).unwrap();
    let wrong_artifact = Artifact::of(&wrong_transaction);
    let decoded = Bundle::decode(wrong_transaction, &other_transaction, &wrong_artifact).unwrap();
    let mut expected = f.expected(&records);
    expected.preimages = &wrong_artifact;
    refusal(prepare(
        &expected,
        &f.envelope,
        &f.public,
        &f.writer,
        decoded,
        head_decode(&f.head),
    ));
    let digest = manifest::sha256_hex(b"independent record");
    let records = [SelectedRecord {
        path: A,
        recorded: Some(&digest),
        writing: None,
    }];
    refusal(f.run(&f.expected(&records)));
}

#[test]
fn bounded_caller_context_refuses_without_echoing_values_or_relaxing_paths() {
    let f = Fixture::ordinary();
    let records = f.selected();
    let too_long = "s".repeat(65);
    let bad_root = f.root.join(".");
    let bad_units = f.root.parent().unwrap();
    for which in 0..8 {
        let mut expected = f.expected(&records);
        match which {
            0 => expected.transaction = &too_long,
            1 => expected.channel = &too_long,
            2 => expected.commit = MARKER,
            3 => expected.root = &bad_root,
            4 => expected.installed_units = bad_units,
            5 => expected.ordinary_enabled = false,
            6 => expected.derivation += 1,
            _ => expected.installed_units = &f.root,
        }
        refusal(f.run(&expected));
    }
    let invalid = "private-plan-value-must-not-escape".repeat(64);
    for field in 0..3 {
        let record = SelectedRecord {
            path: if field == 0 { &invalid } else { A },
            recorded: if field == 1 { Some(&invalid) } else { None },
            writing: if field == 2 { Some(&invalid) } else { None },
        };
        refusal(f.run(&f.expected(&[record])));
    }
    let too_many: Vec<_> = (0..129)
        .map(|_| SelectedRecord {
            path: A,
            recorded: None,
            writing: None,
        })
        .collect();
    refusal(f.run(&f.expected(&too_many)));
}

#[test]
fn ordinary_keep_none_record_and_removed_here_rows_survive_without_qualification() {
    let paths = [
        A,
        B,
        "/etc/lampOS/expression/c",
        "/etc/lampOS/expression/d",
        "/etc/lampOS/expression/e",
    ];
    let settings = vec![
        Setting::new(paths[0], "new"),
        removed(paths[1]),
        Setting::new(paths[2], "same"),
        Setting::new(paths[3], "record"),
        Setting::new(paths[4], "removed-here"),
    ];
    let recorded = BTreeMap::from([
        (paths[2].into(), manifest::sha256_hex(b"same")),
        (paths[4].into(), crate::install::SET_ASIDE.into()),
    ]);
    let mut f = Fixture::new(
        settings,
        &[
            (paths[0], &[0, 255, 1]),
            (paths[2], b"same"),
            (paths[3], b"record"),
        ],
        recorded,
        BTreeMap::new(),
        None,
    );
    f.close();
    let records = f.selected();
    let expected = f.expected(&records);
    let prepared = f.run(&expected).unwrap();
    assert_eq!(prepared.rows.len(), paths.len());
    assert!(matches!(prepared.rows[0].initial, Some(Act::Keep(_))));
    assert_eq!(prepared.rows[1].initial, None);
    assert_eq!(prepared.rows[2].initial, None);
    assert_eq!(prepared.rows[3].initial, Some(Act::Record));
    assert!(matches!(prepared.rows[4].initial, Some(Act::Keep(_))));
    assert_eq!(prepared.projected_order, [0, 3, 4]);
    assert!(!prepared.has_content_changes);
    for (index, row) in prepared.rows.iter().enumerate() {
        assert_eq!(row.signed_index, index);
    }
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
    assert_eq!(prepared.rows[0].initial, Some(Act::Remove));
    assert!(prepared.has_content_changes);
}

#[test]
fn omitted_signed_pins_are_derived_after_later_rows_with_original_source_retained() {
    for head_on in [false, true] {
        let pins = Setting::new(
            PINS,
            &format!(
                r#"{{"head_pads":[{{"name":"head-1","line":41,"active_low":true,"pull":"up"}}],"legacy_head":{}}}"#,
                head_on
            ),
        );
        let old = pins.text.as_ref().unwrap().as_bytes().to_vec();
        let pins_sha = pins.sha256.clone().unwrap();
        let mut f = Fixture::new(
            vec![pins, Setting::new(A, "new")],
            &[(PINS, &old)],
            BTreeMap::from([(PINS.into(), pins_sha)]),
            BTreeMap::new(),
            Some(head_on),
        );
        f.close();
        let records = f.selected();
        let expected = f.expected(&records);
        let prepared = f.run(&expected).unwrap();
        assert_eq!(prepared.rows[0].initial, None);
        assert_eq!(prepared.rows[1].initial, Some(Act::Write));
        assert_eq!(prepared.projected_order, [1, 0]);
        assert_eq!(
            prepared
                .rows
                .iter()
                .map(|row| row.signed_index)
                .collect::<Vec<_>>(),
            [0, 1]
        );
        let effective = prepared.rows[0].effective.as_ref().unwrap();
        assert_eq!(effective.act, Act::Write);
        let derived: Value =
            serde_json::from_str(effective.setting.text.as_deref().unwrap()).unwrap();
        assert_eq!(derived["legacy_head"], json!(!head_on));
        assert_ne!(effective.setting, f.manifest.settings[0]);
        assert_eq!(prepared.manifest.settings[0], f.manifest.settings[0]);
        assert_eq!(prepared.signed_bytes, f.signed);
        assert!(prepared.has_content_changes);
    }
}

#[test]
fn decoder_accepted_missing_late_head_dependency_never_returns_a_partial_plan() {
    let pins = Setting::new(
        PINS,
        r#"{"head_pads":[{"name":"head-1","line":41,"active_low":true,"pull":"up"}],"legacy_head":true}"#,
    );
    let old = pins.text.as_ref().unwrap().as_bytes().to_vec();
    let mut f = Fixture::new(
        vec![Setting::new(A, "staged-first"), pins],
        &[(PINS, &old)],
        BTreeMap::new(),
        BTreeMap::new(),
        Some(true),
    );
    {
        let records = f.selected();
        let expected = f.expected(&records);
        assert!(f.run(&expected).is_ok());
    }
    // Remove a real conditional Body lookup from a successfully captured replay.
    // The remaining wire is independently valid; preparation must not fall back to disk.
    use base64::{Engine, engine::general_purpose::STANDARD};
    let wanted = STANDARD.encode(
        f.root
            .join("etc/lampOS/body/lamp.env")
            .as_os_str()
            .as_encoded_bytes(),
    );
    f.head_change(|value| {
        let texts = value["texts"].as_array_mut().unwrap();
        let before = texts.len();
        texts.retain(|row| row["path"].as_str() != Some(wanted.as_str()));
        assert_eq!(texts.len() + 1, before);
    });
    f.close();
    let records = f.selected();
    refusal(f.run(&f.expected(&records)));
}

#[test]
fn two_decoder_valid_raw_rows_must_agree_even_without_a_head_prefix_fact() {
    for shape in [false, true] {
        let mut f = Fixture::new(
            vec![Setting::new(A, "a"), Setting::new(B, "b")],
            &[(A, b"old-a"), (B, b"old-b")],
            BTreeMap::new(),
            BTreeMap::new(),
            None,
        );
        // The genuine complete policy footprints concern installed units under
        // /etc/systemd, not /etc/lampOS. A positive preparation first proves
        // this is not merely a closed provider missing a later policy lookup.
        {
            let records = f.selected();
            let expected = f.expected(&records);
            assert!(f.run(&expected).is_ok());
        }
        f.raw_change(|value| {
            if shape {
                // Row B claims /etc/lampOS itself is absent, while row A has
                // observed that same prefix as an existing directory.
                value["entries"][1]["before"] = json!({"kind":"missing_ancestor","component":1});
                value["entries"][1]["ancestors"]
                    .as_array_mut()
                    .unwrap()
                    .truncate(2);
            } else {
                let inode = value["entries"][1]["ancestors"][2]["inode"]
                    .as_u64()
                    .unwrap();
                value["entries"][1]["ancestors"][2]["inode"] = json!(inode.checked_add(1).unwrap());
            }
        });
        let bundle = f.bundle();
        let head = head_decode(&f.head);
        // This existing seam intentionally does not promise intra-Bundle consistency.
        head.check_preimages(&f.root, &bundle).unwrap();
        f.close();
        let records = f.selected();
        refusal(f.run(&f.expected(&records)));
    }
}

#[test]
fn reconciliation_refuses_duplicate_foreign_and_non_pins_changes() {
    let settings = [Setting::new(A, "a"), Setting::new(B, "b")];
    let original: Vec<_> = settings
        .iter()
        .map(|setting| Step {
            setting: setting.clone(),
            act: Act::Write,
        })
        .collect();
    let mut valid_rows: Vec<_> = original
        .iter()
        .enumerate()
        .map(|(index, step)| Row {
            signed_index: index,
            initial: Some(step.act.clone()),
            effective: None,
        })
        .collect();
    assert_eq!(
        reconcile(&settings, &mut valid_rows, original.clone()).unwrap(),
        [0, 1]
    );
    for which in 0..4 {
        let mut rows: Vec<_> = original
            .iter()
            .enumerate()
            .map(|(index, step)| Row {
                signed_index: index,
                initial: Some(step.act.clone()),
                effective: None,
            })
            .collect();
        let mut projected = original.clone();
        match which {
            0 => projected[1] = projected[0].clone(),
            1 => projected[0].setting = Setting::new("/etc/lampOS/expression/foreign", "a"),
            2 => projected[0].setting = Setting::new(A, "mutated"),
            _ => projected[0].act = Act::Remove,
        }
        let error = reconcile(&settings, &mut rows, projected).unwrap_err();
        assert_eq!(error.to_string(), "ordinary plan preparation refused");
    }
}

#[test]
fn signature_tampering_refuses_even_with_rebound_envelope_and_unchanged_key_identity() {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let mut f = Fixture::ordinary();
    {
        let records = f.selected();
        let expected = f.expected(&records);
        assert!(f.run(&expected).is_ok());
    }
    let original: Value = serde_json::from_slice(&f.envelope).unwrap();
    let mut tampered = original.clone();
    let mut signature = STANDARD
        .decode(tampered["signature"].as_str().unwrap())
        .unwrap();
    assert_eq!(signature.len(), 64);
    signature[31] ^= 1;
    tampered["signature"] = json!(STANDARD.encode(signature));
    assert_eq!(tampered["key"], original["key"]);
    assert_eq!(tampered["manifest"], original["manifest"]);
    f.envelope = serde_json::to_vec(&tampered).unwrap();
    f.envelope_artifact = Artifact::of(&f.envelope);
    f.bind_refresh_envelope();
    assert!(manifest::open_as(&f.envelope, &f.public, manifest::UPDATER).is_err());
    f.close();
    let records = f.selected();
    let expected = f.expected(&records);
    assert_eq!(expected.trusted_key_sha256, sha(&f.public));
    assert_eq!(expected.envelope.sha256, f.refresh.envelope.sha256);
    assert_eq!(expected.envelope.size, f.refresh.envelope.size);
    refusal(f.run(&expected));
}

#[test]
fn signed_compact_pins_that_expand_past_the_output_cap_return_no_partial_preparation() {
    let pins = Setting::new(
        PINS,
        r#"{"head_pads":[{"name":"head-1","line":41,"active_low":true,"pull":"up"}],"legacy_head":false}"#,
    );
    let old = pins.text.as_ref().unwrap().as_bytes().to_vec();
    let recorded = BTreeMap::from([(PINS.into(), pins.sha256.clone().unwrap())]);
    let mut f = Fixture::new(
        vec![pins, Setting::new(A, "another staged write")],
        &[(PINS, &old)],
        recorded,
        BTreeMap::new(),
        Some(false),
    );
    {
        let records = f.selected();
        let expected = f.expected(&records);
        assert!(f.run(&expected).unwrap().has_content_changes);
    }
    // Match the established ordinary-projector expansion control: compact
    // signed source stays bounded, while pretty indentation exceeds 1 MiB.
    let mut padding = Value::Array(vec![Value::Bool(false); 12_000]);
    for _ in 0..40 {
        padding = json!({"x": padding});
    }
    let mut object: serde_json::Map<String, Value> =
        serde_json::from_str(f.manifest.settings[0].text.as_deref().unwrap()).unwrap();
    object.insert("padding".into(), padding);
    f.manifest.settings[0] = Setting::new(PINS, &serde_json::to_string(&object).unwrap());
    assert!(
        f.manifest
            .settings
            .iter()
            .map(|setting| setting.text.as_ref().map_or(0, String::len))
            .sum::<usize>()
            < manifest::MOST_SETTINGS_BYTES
    );
    f.resign();
    f.bind_refresh_envelope();
    assert!(f.signed.len() < manifest::MOST_MANIFEST_BYTES);
    assert!(u64::try_from(f.envelope.len()).unwrap() < manifest::MOST_ENVELOPE_BYTES);
    let (opened, signed) = manifest::open_as(&f.envelope, &f.public, manifest::UPDATER).unwrap();
    assert_eq!(opened, f.manifest);
    assert_eq!(signed, f.signed);
    let raw =
        writer_settings::capture(&f.root, &f.manifest.settings, &f.recorded, &f.writing).unwrap();
    f.raw = Bundle::from_capture(&f.transaction, &raw)
        .unwrap()
        .encoded()
        .to_vec();
    f.raw_artifact = Artifact::of(&f.raw);
    let initial = apply::plan(
        &f.root,
        &f.manifest.settings,
        &f.recorded,
        &f.writing,
        &BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(initial.len(), 2);
    let (ordinary, frozen) = capture::collect(&f.root, |inputs| {
        let mut steps = initial;
        head_touch::project_with(
            inputs,
            &f.root,
            &f.units,
            &BTreeMap::new(),
            &f.manifest.settings,
            &mut steps,
        )?;
        head_touch::verify_with(inputs, &f.root, &f.units, &BTreeMap::new(), &steps)?;
        Ok(steps)
    })
    .unwrap();
    assert!(
        ordinary
            .iter()
            .find(|step| step.setting.path == PINS)
            .unwrap()
            .setting
            .text
            .as_ref()
            .unwrap()
            .len()
            > 1024 * 1024
    );
    assert_eq!(
        ordinary
            .iter()
            .find(|step| step.setting.path == A)
            .unwrap()
            .act,
        Act::Write
    );
    f.head = codec::encode(&frozen).unwrap();
    f.head_artifact = Artifact::of(&f.head);
    let bundle = f.bundle();
    let head = head_decode(&f.head);
    head.check_preimages(&f.root, &bundle).unwrap();
    f.close();
    let records = f.selected();
    let expected = f.expected(&records);
    // Both valid input decoders and the ordinary evaluator succeeded. The
    // bounded preparation owns and drops its staged vector on output refusal.
    refusal(prepare(
        &expected,
        &f.envelope,
        &f.public,
        &f.writer,
        bundle,
        head,
    ));
}
