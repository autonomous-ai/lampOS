use lamp_controls::{replay, touch_replay};
use lamp_msgs::ControlFault;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(1);
const PROGRAM: &str = env!("CARGO_BIN_EXE_lamp-controls");
const CLOCK: &str = "privacy-test";

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "controls-privacy-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn command(&self) -> Command {
        let mut command = Command::new(PROGRAM);
        command
            .env_remove("LAMP_PRIVACY_CONTROLS")
            .env("LAMP_BUS_ROOT", self.0.join("must-not-exist"));
        command
    }
    fn replay(&self, touch: bool) -> Command {
        let mut command = self.command();
        command
            .args(["replay", "--clock-id", CLOCK, "--config"])
            .arg(fixture("privacy-controls.json"))
            .arg("--input")
            .arg(fixture(if touch {
                "privacy-touch-replay.jsonl"
            } else {
                "privacy-replay.jsonl"
            }));
        command
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}
fn frames(output: &Output) -> Vec<replay::Frame> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::str::from_utf8(&output.stdout)
        .unwrap()
        .lines()
        .map(|row| serde_json::from_str(row).unwrap())
        .collect()
}

#[test]
fn absent_off_and_other_values_preserve_exact_replay_output_and_open_no_bus() {
    let root = Root::new();
    let absent = root.replay(false).output().unwrap();
    assert!(
        frames(&absent)
            .iter()
            .all(|frame| frame.output.state.privacy.is_none())
    );
    // `no-slide` too: its replay is as off.
    for value in ["off", "ON", "true", "", "no-slide"] {
        let output = root
            .replay(false)
            .env("LAMP_PRIVACY_CONTROLS", value)
            .output()
            .unwrap();
        assert_eq!(output.status, absent.status);
        assert_eq!(output.stdout, absent.stdout);
        assert_eq!(output.stderr, absent.stderr);
    }
    assert!(!root.0.join("must-not-exist").exists());
}

#[test]
fn selected_legacy_replay_uses_explicit_wiring_without_opening_the_chip() {
    let root = Root::new();
    let output = root
        .replay(false)
        .env("LAMP_PRIVACY_CONTROLS", "on")
        .arg("--pins")
        .arg(fixture("privacy-pins.json"))
        .output()
        .unwrap();
    let frames = frames(&output);
    assert_eq!(
        frames[0].output.state.privacy.as_ref().unwrap().muted,
        Some(true)
    );
    assert!(
        frames
            .iter()
            .all(|frame| !frame.output.state.enabled && frame.output.gestures.is_empty())
    );
    assert_eq!(
        frames
            .last()
            .unwrap()
            .output
            .state
            .privacy
            .as_ref()
            .unwrap()
            .fault,
        Some(ControlFault::Stale)
    );
    assert!(!root.0.join("must-not-exist").exists());
}

#[test]
fn privacy_only_touch_profile_has_real_controls_frames_without_invented_touch_sources() {
    let root = Root::new();
    let output = root
        .replay(true)
        .env("LAMP_PRIVACY_CONTROLS", "on")
        .args(["--touch", "--pins"])
        .arg(fixture("privacy-pins.json"))
        .output()
        .unwrap();
    let frames = frames(&output);
    assert_eq!(
        frames[0].output.state.privacy.as_ref().unwrap().muted,
        Some(true)
    );
    for frame in &frames {
        frame.output.state.check().unwrap();
        assert!(frame.output.state.buttons.is_empty());
        assert!(!frame.output.state.ready);
    }
    assert_eq!(
        frames
            .last()
            .unwrap()
            .output
            .state
            .privacy
            .as_ref()
            .unwrap()
            .fault,
        Some(ControlFault::Missing)
    );
    assert!(!root.0.join("must-not-exist").exists());
}

#[test]
fn privacy_only_profile_is_not_silently_enabled_when_off() {
    let root = Root::new();
    let output = root
        .replay(true)
        .args(["--touch", "--pins"])
        .arg(fixture("privacy-pins.json"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("needs LAMP_PRIVACY_CONTROLS=on"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!root.0.join("must-not-exist").exists());
}

#[test]
fn a_privacy_section_with_hand_inputs_is_refused_when_off_as_before() {
    let root = Root::new();
    let dir = root.0.join("wiring");
    std::fs::create_dir_all(&dir).unwrap();
    let wiring = dir.join("pins.json");
    let mut pins: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("strip-711-pins-example.json")).unwrap())
            .unwrap();
    pins["privacy"] = serde_json::json!(
        {"chip": "/dev/gpiochip1", "line": 9, "active_low": true, "pull": "up"}
    );
    std::fs::write(&wiring, serde_json::to_vec(&pins).unwrap()).unwrap();
    for value in [None, Some("off"), Some("")] {
        let mut command = root.replay(true);
        command.args(["--touch", "--pins"]).arg(&wiring);
        if let Some(value) = value {
            command.env("LAMP_PRIVACY_CONTROLS", value);
        }
        let output = command.output().unwrap();
        assert!(!output.status.success(), "{value:?}");
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("needs LAMP_PRIVACY_CONTROLS=on"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert!(!root.0.join("must-not-exist").exists());
}

#[test]
fn shadow_and_enable_change_only_existing_authority_not_electrical_evidence() {
    let root = Root::new();
    let run = |enabled| {
        let mut command = root.replay(false);
        command
            .env("LAMP_PRIVACY_CONTROLS", "on")
            .arg("--pins")
            .arg(fixture("privacy-pins.json"));
        if enabled {
            command.arg("--enable");
        }
        frames(&command.output().unwrap())
    };
    let mut shadow = run(false);
    let enabled = run(true);
    for frame in &mut shadow {
        frame.output.state.enabled = true;
    }
    assert_eq!(
        serde_json::to_vec(&shadow).unwrap(),
        serde_json::to_vec(&enabled).unwrap()
    );
}

#[test]
fn privacy_records_have_strict_payload_shape_and_inherit_replay_bounds() {
    let records =
        touch_replay::read(include_bytes!("../../fixtures/privacy-touch-replay.jsonl").as_slice())
            .unwrap();
    assert_eq!(records.len(), 4);
    let mut value: serde_json::Value = serde_json::from_str(
        include_str!("../../fixtures/privacy-touch-replay.jsonl")
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    value["input"]["sample"]["permit"] = true.into();
    assert!(touch_replay::read(serde_json::to_vec(&value).unwrap().as_slice()).is_err());
    assert!(
        touch_replay::read(format!("{}\n", " ".repeat(replay::MAX_LINE_BYTES + 1)).as_bytes())
            .is_err()
    );
}
