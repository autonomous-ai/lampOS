use lamp_bus::Bus;
use lamp_msgs::MoodState;
use std::{
    fs,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Owner(Child);
impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_lamp-mood"));
    command.env_clear().arg("serve");
    command
}

#[test]
fn disabled_binary_preserves_an_existing_bus_byte_for_byte() {
    let root = tempfile::Builder::new()
        .prefix("mof")
        .tempdir_in("/tmp")
        .unwrap();
    fs::write(root.path().join("sentinel"), b"existing bus data\0").unwrap();
    let output = command()
        .args(["--zone", "not/a/zone", "--clock-id", ""])
        .arg("--bus-root")
        .arg(root.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("\"enabled\":false"));
    assert_eq!(
        fs::read(root.path().join("sentinel")).unwrap(),
        b"existing bus data\0"
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn enabled_binary_requires_an_explicit_local_zone() {
    let output = command()
        .args(["--enable", "--clock-id", "test-host"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--zone is required"));
}

#[test]
fn a_live_owner_is_exclusive_and_publishes_no_actuator_topics() {
    let root = tempfile::Builder::new()
        .prefix("mop")
        .tempdir_in("/tmp")
        .unwrap();
    let bus = Bus::new(root.path()).unwrap();
    let mut output = bus
        .subscribe_typed(MoodState::TOPIC, "test", MoodState::TYPE)
        .unwrap();
    let args = [
        "--enable",
        "--zone",
        "UTC",
        "--clock-id",
        "test-host",
        "--run-for-ms",
        "10000",
    ];
    let mut owner = Owner(
        command()
            .args(args)
            .arg("--bus-root")
            .arg(root.path())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let started = Instant::now();
    let state = loop {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "owner never published"
        );
        assert!(
            owner.0.try_wait().unwrap().is_none(),
            "owner exited before publishing"
        );
        if let Some(message) = output.recv(Duration::from_millis(50)).unwrap() {
            break message.parse::<MoodState>().unwrap();
        }
    };
    state.check().unwrap();
    assert_eq!(state.stamp.clock_id, "test-host");
    let second = command()
        .args(args)
        .arg("--bus-root")
        .arg(root.path())
        .output()
        .unwrap();
    assert!(!second.status.success());
    assert!(String::from_utf8_lossy(&second.stderr).contains("already has an owner"));
    for topic in [
        "expression/body",
        "expression/light",
        "body/move",
        "voice/say",
        "light/command",
    ] {
        assert!(
            !root.path().join(topic).exists(),
            "unexpected actuator topic {topic}"
        );
    }
}

#[test]
fn finite_enabled_run_reports_counts_and_can_be_restarted_without_private_state() {
    let root = tempfile::Builder::new()
        .prefix("mor")
        .tempdir_in("/tmp")
        .unwrap();
    let args = [
        "--enable",
        "--zone",
        "UTC",
        "--clock-id",
        "test-host",
        "--run-for-ms",
        "100",
    ];
    for _ in 0..2 {
        let output = command()
            .args(args)
            .arg("--bus-root")
            .arg(root.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["enabled"], true);
        assert!(report["published"].as_u64().unwrap() > 0);
        assert_eq!(report["accepted"], 0);
    }
    assert!(!root.path().join("mood.json").exists());
    assert!(!root.path().join("memory").exists());
}

/// #1670: the product unit's switch. Only `LAMP_MOOD=on` starts Mood as
/// `--enable` does; any other value leaves the bus untouched.
#[test]
fn only_lamp_mood_on_starts_it_as_enable_does() {
    for value in ["", "off", "ON", "1", "true"] {
        let root = tempfile::Builder::new()
            .prefix("mos")
            .tempdir_in("/tmp")
            .unwrap();
        let output = command()
            .env("LAMP_MOOD", value)
            // Bounded, so a run wrongly started ends and fails here.
            .args([
                "--zone",
                "UTC",
                "--clock-id",
                "test-host",
                "--run-for-ms",
                "600",
            ])
            .arg("--bus-root")
            .arg(root.path())
            .output()
            .unwrap();
        assert!(output.status.success(), "{value:?}");
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("\"enabled\":false"),
            "{value:?}"
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0, "{value:?}");
    }
    // On: it runs, says its state on the bus, and ends when its time is up.
    // The lamp's own zone (`system`) is how the product unit runs it.
    let zones: &[&str] = if cfg!(target_os = "linux") {
        &["UTC", "system"]
    } else {
        &["UTC"]
    };
    for zone in zones {
        let root = tempfile::Builder::new()
            .prefix("mon")
            .tempdir_in("/tmp")
            .unwrap();
        let output = command()
            .env("LAMP_MOOD", "on")
            .args([
                "--zone",
                zone,
                "--clock-id",
                "test-host",
                "--run-for-ms",
                "600",
            ])
            .arg("--bus-root")
            .arg(root.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{zone}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("\"enabled\":true"),
            "{zone}"
        );
        let state: MoodState = Bus::new(root.path())
            .unwrap()
            .latest(MoodState::TOPIC)
            .unwrap()
            .expect("a state said")
            .parse()
            .unwrap();
        assert!(state.check().is_ok(), "{zone}");
    }
}
