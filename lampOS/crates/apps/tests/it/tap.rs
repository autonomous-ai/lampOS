//! The owner's tap is the yes (#7415, the product rule of 18 Nov: nothing is
//! asked of the person). lamp-cloud writes the seven-field record from the one
//! `apps.install` or `apps.remove` the phone sent; the helper spends it and
//! goes on. These tests pin that: one yes per operation, every answer a plain
//! `ok` word, no step or question between the tap and the work. A record that
//! differs from the package in any of the seven fields is refused, and that is
//! an integrity check (the bytes are not what the tap named), not a question.
use lamp_apps::ask::{Parts, answer};
use lamp_apps::install::installed_record;
use lamp_apps::layout::Layout;
use lamp_apps::package::{AppPackage, AppSpec, sha256_hex};
use lamp_apps::testing::{FakeCompanions, FakeEffects, FakeYes, StubReader};
use lamp_apps::yes::{Op, YesKey};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};

fn package(version: &str, grants: &[&str], net: bool, logs: bool) -> AppPackage {
    AppPackage {
        app: AppSpec {
            name: "alpha".into(),
            title: "Alpha".into(),
            version: version.into(),
            min_build: 1,
            net,
            logs,
            grants: grants.iter().map(|g| (*g).to_owned()).collect(),
            entry: "app.main".into(),
        },
        files: BTreeMap::from([("src/app/main.py".to_owned(), b"x".to_vec())]),
        skills: vec![],
    }
}

/// What lamp-cloud writes for a tap: the key of the package it fetched.
fn tap(op: Op, package: &AppPackage, bytes: &[u8], grants: &[&str]) -> YesKey {
    YesKey {
        op,
        name: package.app.name.clone(),
        version: package.app.version.clone(),
        sha256: sha256_hex(bytes),
        grants: grants.iter().map(|g| (*g).to_owned()).collect(),
        net: package.app.net,
        logs: package.app.logs,
    }
}

struct Rig {
    _root: tempfile::TempDir,
    layout: Layout,
    fx: FakeEffects,
    skills: FakeCompanions,
    owner: u32,
}

fn rig() -> Rig {
    let root = tempfile::tempdir().unwrap();
    let layout = Layout::under(root.path());
    fs::create_dir_all(&layout.staging).unwrap();
    let probe = layout.staging.join("probe");
    fs::write(&probe, b"").unwrap();
    let owner = fs::metadata(&probe).unwrap().uid();
    fs::remove_file(&probe).unwrap();
    Rig {
        _root: root,
        layout,
        fx: FakeEffects::default(),
        skills: FakeCompanions::default(),
        owner,
    }
}

/// Stages `bytes` the way lamp-cloud does and returns the install line.
fn stage(rig: &Rig, bytes: &[u8]) -> String {
    let file = rig
        .layout
        .staging
        .join(format!("{}.tar", sha256_hex(bytes)));
    fs::write(&file, bytes).unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    json!({"install": {"path": file.display().to_string()}}).to_string()
}

fn ask(rig: &mut Rig, yes: &mut FakeYes, package: &AppPackage, line: &str) -> Value {
    let reader = StubReader::new(package.clone());
    let mut parts = Parts {
        layout: &rig.layout,
        fx: &mut rig.fx,
        skills: &mut rig.skills,
        yes,
        reader: &reader,
        staged_owner: rig.owner,
        build: Some(200),
    };
    answer(true, line, &mut parts)
}

#[test]
fn a_tap_installs_updates_and_removes_with_one_yes_each_and_no_question() {
    let mut rig = rig();

    let first = package("1.0.0", &["speak"], false, false);
    let first_bytes = b"package one".to_vec();
    let mut yes = FakeYes::granting(vec![tap(Op::Install, &first, &first_bytes, &["speak"])]);
    let line = stage(&rig, &first_bytes);
    assert_eq!(
        ask(&mut rig, &mut yes, &first, &line),
        json!({"ok": "installed"})
    );
    assert_eq!(yes.asked, 1, "one yes, taken once");

    // An update that asks for more of everything: the tap's key says the new
    // grants, `net` and `logs` only; nothing else is wanted of the person.
    let second = package("1.1.0", &["speak", "hear"], true, true);
    let second_bytes = b"package two".to_vec();
    let mut yes = FakeYes::granting(vec![tap(Op::Update, &second, &second_bytes, &["hear"])]);
    let line = stage(&rig, &second_bytes);
    assert_eq!(
        ask(&mut rig, &mut yes, &second, &line),
        json!({"ok": "updated"})
    );
    assert_eq!(yes.asked, 1);
    let record = installed_record(&rig.layout, "alpha").unwrap();
    assert_eq!(record.version, "1.1.0");
    assert!(record.net && record.logs);

    // A remove: the key names the installed version and package, no grants.
    let mut yes = FakeYes::granting(vec![YesKey {
        op: Op::Remove,
        name: "alpha".into(),
        version: record.version,
        sha256: record.sha256,
        grants: vec![],
        net: false,
        logs: false,
    }]);
    assert_eq!(
        ask(&mut rig, &mut yes, &second, r#"{"remove":"alpha"}"#),
        json!({"ok": "removed"})
    );
    assert_eq!(yes.asked, 1);
    assert!(!rig.layout.code.join("alpha").exists());
}

#[test]
fn a_record_that_differs_in_any_of_the_seven_fields_is_refused_and_writes_nothing() {
    let pkg = package("1.0.0", &["speak", "hear"], true, true);
    let bytes = b"package bytes".to_vec();
    let good = tap(Op::Install, &pkg, &bytes, &["hear", "speak"]);
    let wrong: Vec<(&str, YesKey)> = vec![
        (
            "op",
            YesKey {
                op: Op::Update,
                ..good.clone()
            },
        ),
        (
            "name",
            YesKey {
                name: "beta".into(),
                ..good.clone()
            },
        ),
        (
            "version",
            YesKey {
                version: "1.0.1".into(),
                ..good.clone()
            },
        ),
        (
            "sha256",
            YesKey {
                sha256: sha256_hex(b"other bytes"),
                ..good.clone()
            },
        ),
        (
            "grants",
            YesKey {
                grants: vec!["speak".into()],
                ..good.clone()
            },
        ),
        (
            "net",
            YesKey {
                net: false,
                ..good.clone()
            },
        ),
        (
            "logs",
            YesKey {
                logs: false,
                ..good.clone()
            },
        ),
    ];
    for (field, key) in wrong {
        let mut rig = rig();
        let line = stage(&rig, &bytes);
        let mut yes = FakeYes::granting(vec![key]);
        let said = ask(&mut rig, &mut yes, &pkg, &line);
        let why = said["refused"].as_str().unwrap_or_default().to_owned();
        assert!(why.starts_with("no yes"), "{field}: {said}");
        assert!(!why.contains('?'), "{field} is a refusal, not a question");
        assert!(rig.fx.calls.is_empty(), "{field}: nothing started");
        assert!(!rig.layout.code.join("alpha").exists(), "{field}: no code");
    }
    // The same bytes with the right record go through, so the refusals above
    // were the record's, not the package's.
    let mut rig = rig();
    let line = stage(&rig, &bytes);
    let mut yes = FakeYes::granting(vec![good]);
    assert_eq!(
        ask(&mut rig, &mut yes, &pkg, &line),
        json!({"ok": "installed"})
    );
}

#[test]
fn a_tap_older_than_its_use_or_used_twice_is_not_a_second_question_either() {
    let mut rig = rig();
    let pkg = package("1.0.0", &[], false, false);
    let bytes = b"package bytes".to_vec();
    let mut yes = FakeYes::granting(vec![tap(Op::Install, &pkg, &bytes, &[])]);
    let line = stage(&rig, &bytes);
    assert_eq!(
        ask(&mut rig, &mut yes, &pkg, &line),
        json!({"ok": "installed"})
    );
    // The yes is spent; asking again has nothing to say but a plain refusal
    // (the app is installed), and the helper asks nobody for another.
    let again = ask(&mut rig, &mut yes, &pkg, &line);
    let why = again["refused"].as_str().unwrap();
    assert!(!why.contains('?'), "{why}");
}
