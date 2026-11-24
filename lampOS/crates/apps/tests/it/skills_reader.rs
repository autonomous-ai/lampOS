//! Nine's FORMAT 2 reader through the helper's own: a package made and signed
//! here with a TEST key (no file is committed), read, mapped and installed.
use base64::{Engine, engine::general_purpose::STANDARD};
use lamp_apps::install::install;
use lamp_apps::layout::Layout;
use lamp_apps::package::{PackageReader, sha256_hex};
use lamp_apps::skills_reader::SkillsReader;
use lamp_apps::testing::{FakeCompanions, FakeEffects, FakeYes};
use lamp_apps::yes::{Op, YesKey};
use ring::signature::{Ed25519KeyPair, KeyPair};

const APP: &[u8] = b"print('hello')\n";

fn key(seed: u8) -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).unwrap()
}

fn public(key: &Ed25519KeyPair) -> [u8; 32] {
    key.public_key().as_ref().try_into().unwrap()
}

/// A signed package of one file, as the Store makes it: the manifest and its
/// signature at the top of the tar, the app's folder beside them.
fn package(signer: &Ed25519KeyPair) -> Vec<u8> {
    package_with(signer, &[])
}

/// The same, with companion skills (each a folder with a skill.toml).
fn package_with(signer: &Ed25519KeyPair, skills: &[&str]) -> Vec<u8> {
    package_built(signer, skills, serde_json::json!({}))
}

/// The same, with more fields in the manifest (`ops`, `views`): `extra`'s keys are set over the sample's.
fn package_built(signer: &Ed25519KeyPair, skills: &[&str], extra: serde_json::Value) -> Vec<u8> {
    let toml = b"name = \"tutor\"\n";
    let mut listed = vec![serde_json::json!(
        {"path": "src/store_test_app.py", "size": APP.len(), "sha256": sha256_hex(APP)}
    )];
    for name in skills {
        listed.push(
            serde_json::json!({"path": format!("skills/{name}/skill.toml"),
            "size": toml.len(), "sha256": sha256_hex(toml)}),
        );
    }
    let mut value = serde_json::json!({
        "format": 2, "kind": "app", "name": "store-test", "title": "Store test",
        "version": "0.1.0", "min_build": 1,
        "runtime": {"name": "python3", "entry": "store_test_app"},
        "grants": ["speak", "hear"], "net": false, "logs": false,
        "phrases": ["start the store test"], "triggers": [], "settings": [],
        "skills": skills, "assets": [],
        "files": listed,
    });
    for (key, field) in extra.as_object().unwrap() {
        value[key] = field.clone();
    }
    let manifest = serde_json::to_vec_pretty(&value).unwrap();
    let signature = serde_json::to_vec(&serde_json::json!({
        "key": lamp_update::manifest::fingerprint(&public(signer)),
        "signature": STANDARD.encode(signer.sign(&manifest).as_ref()),
    }))
    .unwrap();
    let mut builder = tar::Builder::new(Vec::new());
    let mut entries: Vec<(String, Vec<u8>)> = vec![
        ("lamp-skill.json".into(), manifest),
        ("lamp-skill.sig".into(), signature),
        ("store-test/src/store_test_app.py".into(), APP.to_vec()),
    ];
    for name in skills {
        entries.push((
            format!("store-test/skills/{name}/skill.toml"),
            toml.to_vec(),
        ));
    }
    for (path, bytes) in entries.iter().map(|(p, b)| (p.as_str(), b.as_slice())) {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, path, bytes).unwrap();
    }
    builder.into_inner().unwrap()
}

fn reader() -> SkillsReader {
    SkillsReader {
        keys: vec![public(&key(1))],
        tools: false,
    }
}

#[test]
fn a_signed_sample_is_read_and_mapped() {
    let package = reader().read(&package(&key(1))).unwrap();
    assert_eq!(package.app.name, "store-test");
    assert_eq!(package.app.version, "0.1.0");
    assert_eq!(package.app.entry, "store_test_app");
    assert_eq!(package.app.grants, ["speak", "hear"]);
    assert!(!package.app.net && !package.app.logs);
    assert!(package.skills.is_empty());
    assert!(package.files.contains_key("src/store_test_app.py"));
    assert!(package.files.contains_key("lamp-skill.json"));
    lamp_apps::package::paths_ok(&package).unwrap();
    lamp_apps::facts::check(&package.app, Some(200)).unwrap();
}

#[test]
fn another_key_or_a_changed_byte_is_refused() {
    assert!(reader().read(&package(&key(2))).is_err());
    let mut bytes = package(&key(1));
    let at = bytes
        .windows(5)
        .position(|w| w == b"hello")
        .expect("the app's text is in the tar");
    bytes[at] = b'j';
    assert!(reader().read(&bytes).is_err());
    assert!(reader().read(b"not a tar").is_err());
}

#[test]
fn the_helper_installs_the_signed_sample_behind_the_yes() {
    let root = tempfile::tempdir().unwrap();
    let layout = Layout::under(root.path());
    let bytes = package(&key(1));
    let mut yes = FakeYes::granting(vec![YesKey {
        op: Op::Install,
        name: "store-test".into(),
        version: "0.1.0".into(),
        sha256: sha256_hex(&bytes),
        grants: vec!["hear".into(), "speak".into()],
        net: false,
        logs: false,
    }]);
    let (mut fx, mut skills) = (FakeEffects::default(), FakeCompanions::default());
    let done = install(
        &layout,
        &mut fx,
        &mut skills,
        &mut yes,
        &reader(),
        &bytes,
        Some(200),
    )
    .unwrap();
    assert_eq!(done.unit, "lamp-app@store-test.service");
    let app = layout.code.join("store-test/current");
    assert_eq!(
        std::fs::read_to_string(app.join("entry.env")).unwrap(),
        "ENTRY=store_test_app\n"
    );
    assert!(app.join("src/store_test_app.py").is_file());
    assert!(app.join("lamp-skill.json").is_file());
}

#[test]
fn a_package_with_companion_skills_is_refused_before_any_yes_is_asked() {
    let root = tempfile::tempdir().unwrap();
    let layout = Layout::under(root.path());
    let bytes = package_with(&key(1), &["tutor"]);
    let mut yes = FakeYes::granting(vec![YesKey {
        op: Op::Install,
        name: "store-test".into(),
        version: "0.1.0".into(),
        sha256: sha256_hex(&bytes),
        grants: vec!["hear".into(), "speak".into()],
        net: false,
        logs: false,
    }]);
    let (mut fx, mut skills) = (FakeEffects::default(), FakeCompanions::default());
    let error = install(
        &layout,
        &mut fx,
        &mut skills,
        &mut yes,
        &reader(),
        &bytes,
        Some(200),
    )
    .unwrap_err();
    assert!(error.contains("companion skills"), "{error}");
    assert_eq!(
        yes.asked, 0,
        "the yes was asked for a package that cannot be installed"
    );
    assert!(!layout.code.join("store-test").exists());
    assert!(fx.calls.is_empty() && skills.placed.is_empty());
}

#[test]
fn no_keys_refuses_every_package() {
    let none = SkillsReader {
        keys: vec![],
        tools: false,
    };
    assert!(none.read(&package(&key(1))).is_err());
}

/// #7415's app views: the helper reads the manifest with nine's reader, so a package whose `ops` or `views` break a
/// rule is refused here with the reader's own words, before any yes is asked, and a right one is read and installs
/// as one without them does (the helper acts on no view).
#[test]
fn a_package_with_views_is_read_by_the_same_rules_as_the_reader_and_a_bad_one_is_refused() {
    let views = serde_json::json!({
        "ops": ["week", "plan"],
        "views": [
            {"id": "this_week", "kind": "chart", "title": "This week", "op": "week", "style": "bar", "unit": "min"},
            {"id": "my_plan", "kind": "form", "title": "My plan", "op": "plan", "submit": "Save plan",
             "fields": [{"key": "days", "type": "number", "label": "Days", "min": 1, "max": 7, "step": 1}]}
        ],
    });
    let package = reader()
        .read(&package_built(&key(1), &[], views.clone()))
        .unwrap();
    assert_eq!(package.app.name, "store-test");
    lamp_apps::facts::check(&package.app, Some(200)).unwrap();
    for (what, change, words) in [
        (
            "an op no view's own",
            (
                "views",
                serde_json::json!([{"id": "a", "kind": "list", "title": "A", "op": "nope"}]),
            ),
            "not one of the app's ops",
        ),
        (
            "a field of another kind",
            (
                "views",
                serde_json::json!([{"id": "a", "kind": "list", "title": "A", "op": "week", "style": "bar"}]),
            ),
            "a list has only",
        ),
        (
            "an unknown kind",
            (
                "views",
                serde_json::json!([{"id": "a", "kind": "map", "title": "A", "op": "week"}]),
            ),
            "unknown variant",
        ),
    ] {
        let mut bad = views.clone();
        bad[change.0] = change.1;
        let why = reader()
            .read(&package_built(&key(1), &[], bad))
            .expect_err(what);
        assert!(why.contains(words), "{what}: {why}");
    }
}

/// The sample with a tools file (#8386): `tools.json` at the folder's top,
/// named by the manifest's `tools` and listed in `files`.
fn package_with_tools(signer: &Ed25519KeyPair) -> Vec<u8> {
    let tools = br#"{"tools": [{"name": "set_plan", "description": "Set the weekly plan."}]}"#;
    let value = serde_json::json!({
        "format": 2, "kind": "app", "name": "store-test", "title": "Store test",
        "version": "0.1.0", "min_build": 1,
        "runtime": {"name": "python3", "entry": "store_test_app"},
        "grants": ["speak", "hear"], "net": false, "logs": false,
        "phrases": [], "triggers": [], "settings": [], "skills": [], "assets": [],
        "tools": "tools.json",
        "files": [
            {"path": "src/store_test_app.py", "size": APP.len(), "sha256": sha256_hex(APP)},
            {"path": "tools.json", "size": tools.len(), "sha256": sha256_hex(tools)},
        ],
    });
    let manifest = serde_json::to_vec_pretty(&value).unwrap();
    let signature = serde_json::to_vec(&serde_json::json!({
        "key": lamp_update::manifest::fingerprint(&public(signer)),
        "signature": STANDARD.encode(signer.sign(&manifest).as_ref()),
    }))
    .unwrap();
    let mut builder = tar::Builder::new(Vec::new());
    for (path, bytes) in [
        ("lamp-skill.json", manifest.as_slice()),
        ("lamp-skill.sig", signature.as_slice()),
        ("store-test/src/store_test_app.py", APP),
        ("store-test/tools.json", tools.as_slice()),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, path, bytes).unwrap();
    }
    builder.into_inner().unwrap()
}

/// An app with a tools file installs with `LAMP_APP_TOOLS` on, its
/// `tools.json` laid beside its manifest; off, it is refused before any yes
/// is asked, as a lamp before the field refused it.
#[test]
fn a_package_with_a_tools_file_installs_only_with_the_switch_on() {
    let root = tempfile::tempdir().unwrap();
    let layout = Layout::under(root.path());
    let bytes = package_with_tools(&key(1));
    let key_of = || YesKey {
        op: Op::Install,
        name: "store-test".into(),
        version: "0.1.0".into(),
        sha256: sha256_hex(&bytes),
        grants: vec!["hear".into(), "speak".into()],
        net: false,
        logs: false,
    };
    let (mut fx, mut skills) = (FakeEffects::default(), FakeCompanions::default());
    let mut yes = FakeYes::granting(vec![key_of()]);
    let off = install(
        &layout,
        &mut fx,
        &mut skills,
        &mut yes,
        &reader(),
        &bytes,
        Some(200),
    )
    .unwrap_err();
    assert!(off.contains("unknown field `tools`"), "{off}");
    assert_eq!(yes.asked, 0);
    let on = SkillsReader {
        tools: true,
        ..reader()
    };
    let mut yes = FakeYes::granting(vec![key_of()]);
    install(
        &layout,
        &mut fx,
        &mut skills,
        &mut yes,
        &on,
        &bytes,
        Some(200),
    )
    .unwrap();
    let app = layout.code.join("store-test/current");
    assert!(app.join("tools.json").is_file());
    assert!(app.join("src/store_test_app.py").is_file());
}
