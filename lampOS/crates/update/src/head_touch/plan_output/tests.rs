use super::super::{
    Act, BODY_ENV, ConfEntries, OrdinaryInputs, PINS, capture, project_with, verify_with,
};
use super::*;
use serde_json::{Map, Value, json};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

fn put(root: &Path, name: &str, text: &str) {
    let path = root.join(name.trim_start_matches('/'));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn fixture(head: bool) -> (tempfile::TempDir, BTreeMap<String, String>, Setting) {
    let root = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let unit = |name: &str, args: &str, environment: &str| {
        format!("[Service]\nExecStart=/opt/lampOS/{name}/current/lamp-{name} {args}\n{environment}")
    };
    let carried = BTreeMap::from([
        (
            "lamp-controls.service".into(),
            unit("controls", &format!("--pins {PINS}"), ""),
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
    for (name, text) in &carried {
        put(root.path(), &format!("etc/systemd/system/{name}"), text);
    }
    put(root.path(), BODY_ENV, "BODY_POSES=\"1,2,3\"\n");
    fs::create_dir(root.path().join("etc/systemd/system/service.d")).unwrap();
    let setting = Setting::new(
        PINS,
        r#"{"head_pads":[{"name":"head-1","line":41,"active_low":true,"pull":"up"}],"legacy_head":false}"#,
    );
    put(root.path(), PINS, setting.text.as_deref().unwrap());
    (root, carried, setting)
}

#[derive(Debug, PartialEq, Eq)]
enum Request {
    Text(PathBuf),
    Entries(PathBuf),
}
struct Trace {
    requests: Vec<Request>,
}
impl HeadInputs for Trace {
    fn text(&mut self, path: &Path) -> Result<Option<String>, String> {
        self.requests.push(Request::Text(path.to_owned()));
        OrdinaryInputs.text(path)
    }
    fn entries(&mut self, path: &Path) -> Result<Option<ConfEntries>, String> {
        self.requests.push(Request::Entries(path.to_owned()));
        OrdinaryInputs.entries(path)
    }
}

#[test]
fn streaming_pretty_bytes_and_newline_obey_the_exact_utf8_byte_limit() {
    let object: Map<String, Value> =
        json!({"legacy_head":true,"escaped":"a\"b\\c\n\t💡","nested":[false,{"x":17}]})
            .as_object()
            .unwrap()
            .clone();
    let expected = format!("{}\n", serde_json::to_string_pretty(&object).unwrap());
    assert_eq!(text(&object, expected.len()).unwrap(), expected);
    assert_eq!(text(&object, expected.len() - 1).unwrap_err(), REFUSED);
    assert_eq!(text(&object, 0).unwrap_err(), REFUSED);
    assert_eq!(
        crate::manifest::sha256_hex(text(&object, expected.len()).unwrap().as_bytes()),
        crate::manifest::sha256_hex(expected.as_bytes())
    );
    let mut boundary: Map<String, Value> = json!({"x":""}).as_object().unwrap().clone();
    let overhead = serde_json::to_string_pretty(&boundary).unwrap().len() + 1;
    boundary.insert(
        "x".into(),
        Value::String("x".repeat(MOST_DERIVED_BYTES - overhead)),
    );
    assert_eq!(
        text(&boundary, MOST_DERIVED_BYTES).unwrap().len(),
        MOST_DERIVED_BYTES
    );
    let Value::String(value) = boundary.get_mut("x").unwrap() else {
        unreachable!()
    };
    value.push('x');
    assert_eq!(text(&boundary, MOST_DERIVED_BYTES).unwrap_err(), REFUSED);
}

#[test]
fn refusing_an_append_happens_before_buffer_growth_or_partial_write() {
    let mut out = Output {
        bytes: Vec::with_capacity(3),
        limit: 0,
    };
    out.limit = out.bytes.capacity();
    out.bytes.resize(out.limit, b'x');
    let before = out.bytes.clone();
    let capacity = out.bytes.capacity();
    assert!(out.write_all(b"y").is_err());
    assert_eq!(out.bytes, before);
    assert_eq!(out.bytes.capacity(), capacity);
}

#[test]
fn ordinary_policy_bytes_hashes_and_request_order_match_all_eight_closed_replays() {
    for head in [false, true] {
        for omitted in [false, true] {
            for use_carried in [false, true] {
                let (root, all, setting) = fixture(head);
                let base = root.path().to_owned();
                let units = base.join("etc/systemd/system");
                let carried = if use_carried { all } else { BTreeMap::new() };
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
                let mut old = Trace {
                    requests: Vec::new(),
                };
                project_with(&mut old, &base, &units, &carried, &settings, &mut ordinary).unwrap();
                let mut new = Trace {
                    requests: Vec::new(),
                };
                let bounded = project(
                    &mut new,
                    &base,
                    &units,
                    &carried,
                    &settings,
                    initial.clone(),
                )
                .unwrap();
                assert_eq!(bounded, ordinary);
                assert_eq!(new.requests, old.requests);
                verify_with(&mut OrdinaryInputs, &base, &units, &carried, &bounded).unwrap();
                let (expected, facts) = capture::collect(&base, |inputs| {
                    let steps =
                        project(inputs, &base, &units, &carried, &settings, initial.clone())?;
                    verify_with(inputs, &base, &units, &carried, &steps)?;
                    Ok(steps)
                })
                .unwrap();
                let bytes = capture::codec::encode(&facts).unwrap();
                let sha: [u8; 32] = ring::digest::digest(&ring::digest::SHA256, &bytes)
                    .as_ref()
                    .try_into()
                    .unwrap();
                let mut saved =
                    capture::codec::decode(&bytes, &sha, bytes.len().try_into().unwrap()).unwrap();
                root.close().unwrap();
                let actual =
                    project(&mut saved, &base, &units, &carried, &settings, initial).unwrap();
                verify_with(&mut saved, &base, &units, &carried, &actual).unwrap();
                assert_eq!(actual, expected);
                assert_eq!(actual, ordinary);
                assert_eq!(saved.as_bytes(), bytes);
            }
        }
    }
}

#[test]
fn compact_input_that_expands_past_the_cap_refuses_only_the_new_plan_path() {
    let (root, carried, mut setting) = fixture(false);
    // A policy/formatter fixture, not a signed/admitted plan. This compact
    // object fits the signed-text size cap but pretty indentation expands it.
    let mut padding = Value::Array(vec![Value::Bool(false); 12_000]);
    for _ in 0..40 {
        padding = json!({"x":padding});
    }
    let mut object: Map<String, Value> =
        serde_json::from_str(setting.text.as_deref().unwrap()).unwrap();
    object.insert("padding".into(), padding);
    setting = Setting::new(PINS, &serde_json::to_string(&object).unwrap());
    assert!(setting.text.as_ref().unwrap().len() < crate::manifest::MOST_SETTINGS_BYTES);
    put(root.path(), PINS, setting.text.as_deref().unwrap());
    let units = root.path().join("etc/systemd/system");
    let settings = [setting];
    let initial = vec![Step {
        setting: settings[0].clone(),
        act: Act::Write,
    }];
    let mut ordinary = initial.clone();
    project_with(
        &mut OrdinaryInputs,
        root.path(),
        &units,
        &carried,
        &settings,
        &mut ordinary,
    )
    .unwrap();
    assert!(ordinary[0].setting.text.as_ref().unwrap().len() > MOST_DERIVED_BYTES);
    verify_with(
        &mut OrdinaryInputs,
        root.path(),
        &units,
        &carried,
        &ordinary,
    )
    .unwrap();
    let refused = project(
        &mut OrdinaryInputs,
        root.path(),
        &units,
        &carried,
        &settings,
        initial,
    )
    .unwrap_err();
    assert_eq!(refused, REFUSED);
}

#[test]
fn keep_step_has_no_derived_output_and_moves_oversized_existing_text_without_copying() {
    let (root, carried, setting) = fixture(false);
    let mut kept = setting.clone();
    kept.text = Some("x".repeat(MOST_DERIVED_BYTES + 1));
    let pointer = kept.text.as_ref().unwrap().as_ptr();
    // Deliberately oversized existing input demonstrates the output-only
    // contract; this is not authentication or eligibility of a complete plan.
    let steps = project(
        &mut OrdinaryInputs,
        root.path(),
        &root.path().join("etc/systemd/system"),
        &carried,
        &[setting],
        vec![Step {
            setting: kept,
            act: Act::Keep("local fixture".into()),
        }],
    )
    .unwrap();
    assert_eq!(steps[0].act, Act::Keep("local fixture".into()));
    assert_eq!(
        steps[0].setting.text.as_ref().unwrap().len(),
        MOST_DERIVED_BYTES + 1
    );
    assert_eq!(steps[0].setting.text.as_ref().unwrap().as_ptr(), pointer);
}

struct FailLate {
    pins: PathBuf,
    reads: usize,
}
impl HeadInputs for FailLate {
    fn text(&mut self, path: &Path) -> Result<Option<String>, String> {
        if path == self.pins {
            self.reads += 1;
            if self.reads == 2 {
                return Err("private-provider-marker".into());
            }
        }
        OrdinaryInputs.text(path)
    }
    fn entries(&mut self, path: &Path) -> Result<Option<ConfEntries>, String> {
        OrdinaryInputs.entries(path)
    }
}

#[test]
fn late_provider_failure_returns_no_owned_partial_plan_or_private_diagnostic() {
    let (root, carried, setting) = fixture(false);
    let mut inputs = FailLate {
        pins: root.path().join(PINS.trim_start_matches('/')),
        reads: 0,
    };
    let result = project(
        &mut inputs,
        root.path(),
        &root.path().join("etc/systemd/system"),
        &carried,
        &[setting],
        Vec::new(),
    );
    assert_eq!(inputs.reads, 2);
    assert_eq!(result.unwrap_err(), REFUSED);
}
