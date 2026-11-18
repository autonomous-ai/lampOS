//! Format 2, an app's package (#7415 piece 1, `docs/apps.md`): read and
//! checked in memory, nothing written. Each refusal is a test of its own
//! words; a package that is right reads, with its files and its companion
//! skills told apart.
use lamp_skills::app_package::{self, read_app_package_bytes};
use lamp_skills::package::{self, MANIFEST, SIGNATURE, sha256_hex};
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{Value, json};

fn key(which: u8) -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed_unchecked(&[which; 32]).unwrap()
}

fn public(key: &Ed25519KeyPair) -> [u8; 32] {
    key.public_key().as_ref().try_into().unwrap()
}

/// A WAV clip of `frames` frames, mono 16-bit at 24 kHz, quiet enough.
fn wav(frames: u32, channels: u16) -> Vec<u8> {
    let spec = hound::WavSpec {
        channels,
        sample_rate: 24_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut out = std::io::Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut out, spec).unwrap();
        for n in 0..frames * u32::from(channels) {
            writer
                .write_sample(((n as f32 / 20.0).sin() * 800.0) as i16)
                .unwrap();
        }
        writer.finalize().unwrap();
    }
    out.into_inner()
}

type Files = Vec<(String, Vec<u8>)>;

fn good_files() -> Files {
    vec![
        ("src/lamp_learn/__init__.py".into(), b"".to_vec()),
        ("src/lamp_learn/app.py".into(), b"print('hello')\n".to_vec()),
        ("assets/ok.wav".into(), wav(2_400, 1)),
        (
            "skills/english-tutor/skill.toml".into(),
            b"name = \"english-tutor\"\n".to_vec(),
        ),
    ]
}

fn good_manifest(files: &Files) -> Value {
    json!({
        "format": 2,
        "kind": "app",
        "name": "english-lessons",
        "title": "English lessons",
        "version": "1.2.0",
        "min_build": 143,
        "runtime": {"name": "python3", "entry": "lamp_learn.app"},
        "grants": ["speak", "hear", "light", "body", "sound", "skills"],
        "net": false,
        "logs": true,
        "phrases": ["start my english lesson"],
        "triggers": ["start my english lesson", "begin the english lesson"],
        "settings": [
            {"key": "level", "type": "choice", "label": "Level",
             "options": [{"value": "a1", "label": "A1"}, {"value": "a2", "label": "A2"}],
             "default": "a1", "hint": "optional"},
            {"key": "answer_s", "type": "number", "label": "Seconds to answer",
             "min": 3, "max": 60, "unit": "s", "default": 15},
            {"key": "name", "type": "text", "label": "Learner's name", "max": 40, "default": ""},
            {"key": "hints", "type": "toggle", "label": "Show hints", "default": true}
        ],
        "skills": ["english-tutor"],
        "assets": ["assets/ok.wav"],
        "files": files_entries(files),
    })
}

fn files_entries(files: &Files) -> Value {
    Value::Array(
        files
            .iter()
            .map(|(path, data)| json!({"path": path, "size": data.len(), "sha256": sha256_hex(data)}))
            .collect(),
    )
}

/// A package: `manifest` signed by `signer` (None: unsigned), the tar's
/// folder `folder`, with `files` and `extra` beside the manifest.
fn tar(folder: &str, manifest: &Value, files: &Files, signer: Option<&Ed25519KeyPair>) -> Vec<u8> {
    let manifest = serde_json::to_vec(manifest).unwrap();
    let mut builder = tar::Builder::new(Vec::new());
    // As in format 1: the manifest and its signature beside the folder,
    // every other file inside it.
    let mut put = |path: &str, data: &[u8], beside: bool| {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        let path = if beside {
            path.to_owned()
        } else {
            format!("{folder}/{path}")
        };
        builder.append_data(&mut header, path, data).unwrap();
    };
    put(MANIFEST, &manifest, true);
    if let Some(signer) = signer {
        put(
            SIGNATURE,
            &package::signature_for(&manifest, signer).unwrap(),
            true,
        );
    }
    for (path, data) in files {
        put(path, data, false);
    }
    builder.into_inner().unwrap()
}

fn good() -> Vec<u8> {
    let files = good_files();
    tar(
        "english-lessons",
        &good_manifest(&files),
        &files,
        Some(&key(1)),
    )
}

fn read(bytes: &[u8]) -> Result<app_package::AppPackage, String> {
    read_app_package_bytes(bytes, &[public(&key(1))])
}

/// A package whose manifest and files are changed by `change` before it is
/// signed, so only that rule is what refuses it.
fn changed(change: impl FnOnce(&mut Value, &mut Files)) -> Result<app_package::AppPackage, String> {
    let mut files = good_files();
    let mut manifest = good_manifest(&files);
    change(&mut manifest, &mut files);
    read(&tar("english-lessons", &manifest, &files, Some(&key(1))))
}

/// As `changed`, then the manifest's `files` rebuilt from the changed files.
fn refiled(change: impl FnOnce(&mut Value, &mut Files)) -> Result<app_package::AppPackage, String> {
    changed(|manifest, files| {
        change(manifest, files);
        manifest["files"] = files_entries(files);
    })
}

fn refused(result: Result<app_package::AppPackage, String>, words: &str) {
    match result {
        Ok(_) => panic!("read, and should be refused ({words})"),
        Err(why) => assert!(why.contains(words), "{why:?} lacks {words:?}"),
    }
}

#[test]
fn a_right_package_reads_with_its_files_and_companion_skills_told_apart() {
    let bytes = good();
    let package = read(&bytes).unwrap();
    assert_eq!(package.manifest.name, "english-lessons");
    assert_eq!(package.manifest.min_build, 143);
    assert_eq!(package.manifest.runtime.entry, "lamp_learn.app");
    assert!(package.manifest.logs && !package.manifest.net);
    assert_eq!(
        package.files.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "assets/ok.wav",
            "src/lamp_learn/__init__.py",
            "src/lamp_learn/app.py"
        ]
    );
    assert_eq!(package.skills.len(), 1);
    assert!(package.skills["english-tutor"].contains_key("skill.toml"));
    assert_eq!(package.sha256, sha256_hex(&bytes));
    assert_eq!(
        package.signed_by,
        lamp_update::manifest::fingerprint(&public(&key(1)))
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&package.manifest_bytes).unwrap()["name"],
        "english-lessons"
    );
}

#[test]
fn an_unsigned_or_wrongly_signed_package_is_refused_before_the_manifest_is_read() {
    let files = good_files();
    let manifest = good_manifest(&files);
    refused(
        read(&tar("english-lessons", &manifest, &files, None)),
        "carries no lamp-skill.sig",
    );
    refused(
        read(&tar("english-lessons", &manifest, &files, Some(&key(2)))),
        "signed with key",
    );
    // A manifest that is not even JSON is refused by its signature first.
    let mut builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_size(3);
    header.set_mode(0o644);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_cksum();
    builder
        .append_data(&mut header, "lamp-skill.json", &b"???"[..])
        .unwrap();
    refused(
        read(&builder.into_inner().unwrap()),
        "carries no lamp-skill.sig",
    );
}

#[test]
fn a_format_one_lamp_says_what_it_always_said_of_a_format_two_package() {
    let why = package::read_package_bytes(&good(), None).unwrap_err();
    assert!(
        why.contains("format 2 is not one this lamp reads (1)"),
        "{why}"
    );
}

#[test]
fn a_skill_package_is_no_app() {
    let manifest = json!({"format": 1, "name": "x-skill", "version": "1.0.0", "files": []});
    let why = read(&tar("x-skill", &manifest, &Vec::new(), Some(&key(1)))).unwrap_err();
    assert!(why.contains("lamp-skill.json"), "{why}");
}

#[test]
fn the_manifests_own_rules_each_refuse_in_their_own_words() {
    let name = |value: &str| {
        let value = value.to_owned();
        move |manifest: &mut Value, _: &mut Files| manifest["name"] = Value::String(value)
    };
    for bad in ["English", "1lessons", "a_b", "", &"a".repeat(25)] {
        refused(changed(name(bad)), "is not an app's name");
    }
    refused(
        changed(|m, _| m["kind"] = "skill".into()),
        "kind is \"app\"",
    );
    refused(changed(|m, _| m["title"] = "".into()), "a title is 1 to 40");
    refused(
        changed(|m, _| m["title"] = "x".repeat(41).into()),
        "a title is 1 to 40",
    );
    refused(
        changed(|m, _| m["version"] = "1.0".into()),
        "MAJOR.MINOR.PATCH",
    );
    refused(changed(|m, _| m["min_build"] = 0.into()), "min_build");
    refused(
        changed(|m, _| m["runtime"]["name"] = "node".into()),
        "runtime.name",
    );
    refused(
        changed(|m, _| m["runtime"]["entry"] = "bad entry".into()),
        "dotted module path",
    );
    refused(
        changed(|m, _| m["runtime"]["entry"] = "Lamp.App".into()),
        "dotted module path",
    );
    // Unknown fields are refused at every level.
    refused(changed(|m, _| m["extra"] = 1.into()), "unknown field");
    refused(
        changed(|m, _| m["runtime"]["extra"] = 1.into()),
        "unknown field",
    );
    refused(
        changed(|m, _| m["settings"][3]["extra"] = 1.into()),
        "unknown field",
    );
}

#[test]
fn grants_are_the_doors_fourteen_and_nothing_else() {
    refused(
        changed(|m, _| m["grants"] = json!(["speak", "net"])),
        "not one of the door's",
    );
    refused(
        changed(|m, _| m["grants"] = json!(["speak", "logs"])),
        "not one of the door's",
    );
    refused(
        changed(|m, _| m["grants"] = json!(["speak", "mail-read"])),
        "not one of the door's",
    );
    refused(
        changed(|m, _| m["grants"] = json!(["speak", "speak", "hear"])),
        "twice",
    );
    // No grants at all is an app that asks for nothing, with no hear: its
    // phrases and triggers are then refused.
    refused(
        changed(|m, _| m["grants"] = json!(["speak"])),
        "need the hear grant",
    );
    assert!(
        changed(|m, _| {
            m["grants"] = json!([]);
            m["phrases"] = json!([]);
            m["triggers"] = json!([]);
        })
        .is_ok()
    );
    for grant in app_package::GRANTS {
        assert!(
            changed(move |m, _| m["grants"] = json!([grant, "hear"])).is_ok() || grant == "hear",
            "{grant}"
        );
    }
}

#[test]
fn phrases_and_triggers_hold_the_doors_rule_and_their_caps() {
    refused(
        changed(|m, _| m["phrases"] = json!(["short"])),
        "fewer than 8 letters",
    );
    refused(
        changed(|m, _| m["phrases"] = json!(["a".repeat(65)])),
        "more than 64 bytes",
    );
    refused(
        changed(|m, _| m["phrases"] = json!(["start my lesson", "Start my Lesson"])),
        "twice",
    );
    let many: Vec<String> = (0..17)
        .map(|n| format!("phrase number {}", "a".repeat(n + 1)))
        .collect();
    refused(
        changed(move |m, _| m["phrases"] = json!(many)),
        "phrases: at most 16",
    );
    let nine: Vec<String> = (0..9)
        .map(|n| format!("trigger number {}", "a".repeat(n + 1)))
        .collect();
    refused(
        changed(move |m, _| m["triggers"] = json!(nine)),
        "triggers: at most 8",
    );
    refused(
        changed(|m, _| m["triggers"] = json!(["abc def"])),
        "fewer than 8 letters",
    );
    // A trigger may also be a phrase: the voice delivers it once.
    assert!(changed(|m, _| m["phrases"] = json!(["start my english lesson"])).is_ok());
    // `triggers` is required, as the first builder's text has it.
    refused(
        changed(|m, _| {
            m.as_object_mut().unwrap().remove("triggers");
        }),
        "triggers",
    );
}

#[test]
fn settings_hold_each_types_rules() {
    let setting = |value: Value| move |m: &mut Value, _: &mut Files| m["settings"] = json!([value]);
    let ok = |value: Value| changed(setting(value)).is_ok();
    assert!(ok(
        json!({"key": "n", "type": "text", "label": "N", "max": 200, "default": "abc"})
    ));
    refused(
        changed(setting(
            json!({"key": "n", "type": "text", "label": "N", "default": ""}),
        )),
        "needs max",
    );
    refused(
        changed(setting(
            json!({"key": "n", "type": "text", "label": "N", "max": 201, "default": ""}),
        )),
        "max is a whole number, 1 to 200",
    );
    refused(
        changed(setting(
            json!({"key": "n", "type": "text", "label": "N", "max": 2, "default": "abc"}),
        )),
        "longer than max",
    );
    refused(
        changed(setting(
            json!({"key": "n", "type": "number", "label": "N", "min": 5, "max": 5, "default": 5}),
        )),
        "min is below max",
    );
    refused(
        changed(setting(
            json!({"key": "n", "type": "number", "label": "N", "min": 1, "max": 5, "default": 9}),
        )),
        "outside min and max",
    );
    refused(
        changed(setting(
            json!({"key": "n", "type": "toggle", "label": "N", "default": "yes"}),
        )),
        "true or false",
    );
    refused(
        changed(setting(
            json!({"key": "n", "type": "toggle", "label": "N", "default": true, "max": 3}),
        )),
        "only key, label, hint and default",
    );
    let options = |n: usize| {
        json!(
            (0..n)
                .map(|i| json!({"value": format!("v{i}"), "label": "L"}))
                .collect::<Vec<_>>()
        )
    };
    refused(
        changed(setting(
            json!({"key": "n", "type": "choice", "label": "N", "options": options(1), "default": "v0"}),
        )),
        "2 to 16 options",
    );
    refused(
        changed(setting(
            json!({"key": "n", "type": "choice", "label": "N", "options": options(17), "default": "v0"}),
        )),
        "2 to 16 options",
    );
    assert!(ok(
        json!({"key": "n", "type": "choice", "label": "N", "options": options(16), "default": "v15"})
    ));
    refused(
        changed(setting(
            json!({"key": "n", "type": "choice", "label": "N", "options": options(2), "default": "zz"}),
        )),
        "not one of the options",
    );
    refused(
        changed(setting(json!({"key": "n", "type": "choice", "label": "N",
            "options": [{"value": "A", "label": "L"}, {"value": "b", "label": "L"}], "default": "b"}))),
        "[a-z0-9][a-z0-9_-]",
    );
    refused(
        changed(setting(
            json!({"key": "Bad Key", "type": "toggle", "label": "N", "default": true}),
        )),
        "a key is 1 to 32",
    );
    refused(
        changed(|m, _| {
            m["settings"] = json!([
                {"key": "k", "type": "toggle", "label": "A", "default": true},
                {"key": "k", "type": "toggle", "label": "B", "default": false}
            ])
        }),
        "twice",
    );
    let sixteen_and_one: Vec<Value> = (0..17)
        .map(|n| json!({"key": format!("k{n}"), "type": "toggle", "label": "L", "default": true}))
        .collect();
    refused(
        changed(move |m, _| m["settings"] = json!(sixteen_and_one)),
        "at most 16 settings",
    );
    refused(
        changed(setting(
            json!({"key": "n", "type": "toggle", "label": "N", "hint": "h".repeat(121), "default": true}),
        )),
        "a hint is 1 to 120",
    );
    refused(
        changed(setting(
            json!({"key": "n", "type": "mystery", "label": "N", "default": true}),
        )),
        "unknown variant",
    );
}

#[test]
fn files_are_exactly_the_listed_ones_under_src_assets_and_skills() {
    refused(
        refiled(|_, files| files.push(("README.md".into(), b"x".to_vec()))),
        "under src/, assets/ and skills/",
    );
    refused(
        changed(|_, files| files.push(("src/lamp_learn/extra.py".into(), b"x".to_vec()))),
        "is in the package and not in",
    );
    refused(
        changed(|m, _| {
            let entry = m["files"][0].clone();
            m["files"].as_array_mut().unwrap().push(entry);
        }),
        "twice",
    );
    refused(
        changed(|m, _| {
            m["files"]
                .as_array_mut()
                .unwrap()
                .push(json!({"path": "src/gone.py", "size": 1, "sha256": "0".repeat(64)}))
        }),
        "is in lamp-skill.json and not in the package",
    );
    refused(
        changed(|m, _| m["files"][1]["sha256"] = "0".repeat(64).into()),
        "size or sha256",
    );
    refused(
        changed(|_, files| files[1].1 = b"print('changed')\n".to_vec()),
        "size or sha256",
    );
    refused(
        changed(|m, _| {
            m["files"]
                .as_array_mut()
                .unwrap()
                .push(json!({"path": "lamp-skill.json", "size": 1, "sha256": "0".repeat(64)}))
        }),
        "not one of its files",
    );
    // A path deeper than 8 parts, and one with a dot part.
    refused(
        refiled(|_, files| files.push(("src/a/b/c/d/e/f/g/h.py".into(), b"x".to_vec()))),
        "at most 8 parts",
    );
    refused(
        refiled(|_, files| files.push(("src/.hidden.py".into(), b"x".to_vec()))),
        "none beginning with a dot",
    );
    // The package's folder is the app's name.
    let files = good_files();
    refused(
        read(&tar(
            "other-app",
            &good_manifest(&files),
            &files,
            Some(&key(1)),
        )),
        "the package's folder is",
    );
}

#[test]
fn the_entry_module_is_in_src() {
    refused(
        refiled(|m, _| m["runtime"]["entry"] = "lamp_learn.missing".into()),
        "is not in src/",
    );
    // A package's __init__ also is a module.
    assert!(refiled(|m, _| m["runtime"]["entry"] = "lamp_learn".into()).is_ok());
}

#[test]
fn companion_skills_are_named_and_present_both_ways() {
    refused(
        refiled(|m, _| m["skills"] = json!([])),
        "skills and the folders under skills/ differ",
    );
    refused(
        refiled(|m, _| m["skills"] = json!(["english-tutor", "ghost"])),
        "skills and the folders under skills/ differ",
    );
    refused(
        refiled(|_, files| files[3].0 = "skills/english-tutor/notes.txt".into()),
        "no skill.toml",
    );
    refused(
        refiled(|m, _| m["skills"] = json!(["Bad_Name"])),
        "is not a skill's name",
    );
}

#[test]
fn assets_are_clips_the_list_and_the_files_agree_on() {
    refused(
        refiled(|m, _| m["assets"] = json!([])),
        "assets and the files under assets/ differ",
    );
    refused(
        refiled(|m, _| m["assets"] = json!(["assets/ok.wav", "assets/ok.wav"])),
        "twice",
    );
    refused(
        refiled(|m, _| m["assets"] = json!(["src/x.wav"])),
        "is under assets/ and ends .wav",
    );
    refused(
        refiled(|_, files| {
            files[2].1 = b"not a wav".to_vec();
        }),
        "not a WAV file",
    );
    // Stereo, and past ten seconds.
    refused(
        refiled(|_, files| files[2].1 = wav(2_400, 2)),
        "not 16-bit mono PCM",
    );
    refused(
        refiled(|_, files| files[2].1 = wav(app_package::ASSET_MOST_FRAMES + 1, 1)),
        "frames",
    );
    assert!(
        refiled(|_, files| files[2].1 = wav(app_package::ASSET_MOST_FRAMES, 1)).is_ok(),
        "ten seconds exactly is a clip"
    );
    let twenty_one: Vec<String> = (0..21).map(|n| format!("assets/c{n}.wav")).collect();
    refused(
        refiled(move |m, files| {
            for path in &twenty_one {
                files.push((path.clone(), wav(2_400, 1)));
            }
            m["assets"] = json!(twenty_one);
        }),
        "at most 20 assets",
    );
}

#[test]
fn the_name_rules_are_the_texts_and_a_clip_may_be_quiet() {
    assert!(app_package::app_name_ok("a"));
    assert!(app_package::app_name_ok("english-lessons"));
    assert!(app_package::app_name_ok(&"a".repeat(24)));
    assert!(!app_package::app_name_ok(&"a".repeat(25)));
    assert!(!app_package::app_name_ok("-a"));
    assert!(!app_package::app_name_ok("a_b"));
    assert_eq!(app_package::GRANTS.len(), 14);
}
