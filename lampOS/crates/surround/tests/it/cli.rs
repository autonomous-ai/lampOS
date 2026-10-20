use lamp_surround::atomic;
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    os::unix::{fs::PermissionsExt, net::UnixStream},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let root = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(fs::canonicalize(std::env::temp_dir()).unwrap())
        .unwrap();
    let settings = root.path().join("settings");
    atomic::ensure_private_dir(&settings).unwrap();
    let mut document: Value =
        serde_json::from_str(include_str!("../../fixtures/surround.json")).unwrap();
    document["state_dir"] = root.path().join("operations").to_str().unwrap().into();
    document["configuration_dir"] = root.path().join("configuration").to_str().unwrap().into();
    document["socket"] = root
        .path()
        .join("run/operator.sock")
        .to_str()
        .unwrap()
        .into();
    document["operator_code_file"] = settings.join("operator.key").to_str().unwrap().into();
    document["operator_origin"] = "https://lamp.example".into();
    atomic::write(&settings.join("operator.key"), &[b'a'; 64], 128).unwrap();
    let path = settings.join("surround.json");
    atomic::write(&path, &serde_json::to_vec(&document).unwrap(), 32768).unwrap();
    (root, path)
}
fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lamp-surround"))
}
fn alter(path: &std::path::Path, change: impl FnOnce(&mut Value)) {
    let mut doc: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    change(&mut doc);
    atomic::write(path, &serde_json::to_vec(&doc).unwrap(), 32768).unwrap();
}

#[test]
fn help_bad_flags_and_private_config_errors_have_no_side_effects() {
    assert!(command().arg("--help").output().unwrap().status.success());
    for args in [
        vec!["serve"],
        vec!["clock", "--config", "not-allowed"],
        vec!["clock", "--clock-id", "a", "--clock-id", "b"],
        vec!["serve", "--config", "absent", "--listen-loopback", "0"],
    ] {
        assert!(!command().args(args).output().unwrap().status.success());
    }
    let (root, path) = fixture();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(
        !command()
            .arg("doctor")
            .arg("--config")
            .arg(path)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn doctor_and_plan_are_explicitly_read_only_even_for_an_enabled_configuration() {
    let (root, path) = fixture();
    alter(&path, |doc| {
        doc["enabled"] = true.into();
        doc["startup"]["enabled"] = true.into();
        doc["startup"]["components"] = json!([{"component":"voice","configured":true,"binary_available":true,"calibration":"not_required","missing_dependencies":[]}]);
    });
    let out = command()
        .arg("doctor")
        .arg("--config")
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success());
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["owner_connected"], false);
    assert_eq!(doc["readiness_proved"], false);
    for (enabled, expected) in [(false, 0), (true, 1)] {
        let mut cmd = command();
        cmd.arg("plan")
            .arg("--config")
            .arg(&path)
            .args(["--clock-id", "written-host"]);
        if enabled {
            cmd.arg("--enable");
        }
        let out = cmd.output().unwrap();
        assert!(out.status.success());
        let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(doc["actions_executed"], 0);
        assert_eq!(doc["launches"].as_array().unwrap().len(), expected);
    }
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn serve_without_both_switches_opens_page_but_does_not_create_maintenance_state() {
    for config_enabled in [false, true] {
        let (root, path) = fixture();
        alter(&path, |doc| doc["enabled"] = config_enabled.into());
        let mut cmd = command();
        cmd.arg("serve")
            .arg("--config")
            .arg(&path)
            .args(["--clock-id", "host-cli-fixture", "--seconds", "1"])
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        if !config_enabled {
            cmd.arg("--enable");
        }
        let child = cmd.spawn().unwrap();
        let socket = root.path().join("run/operator.sock");
        let began = Instant::now();
        let mut stream = loop {
            match UnixStream::connect(&socket) {
                Ok(stream) => break stream,
                Err(_) if began.elapsed() < Duration::from_secs(5) => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(error) => panic!("private fixture listener unavailable: {error}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream
            .write_all(
                b"GET /api/status HTTP/1.1\r\nHost: lamp.example\r\nConnection: close\r\n\r\n",
            )
            .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 401"));
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!root.path().join("operations").exists());
        assert!(!root.path().join("configuration").exists());
        assert!(!socket.exists());
        assert!(!String::from_utf8_lossy(&output.stderr).contains(&"a".repeat(64)));
    }
}

#[test]
fn invalid_loopback_origin_fails_before_any_adapter_or_operation_is_created() {
    let (root, path) = fixture();
    alter(&path, |doc| doc["enabled"] = true.into());
    let out = command()
        .arg("serve")
        .arg("--config")
        .arg(path)
        .args([
            "--enable",
            "--clock-id",
            "host-cli-fixture",
            "--listen-loopback",
            "8877",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(!root.path().join("operations").exists());
    assert!(!root.path().join("configuration").exists());
}
