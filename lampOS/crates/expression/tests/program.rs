use lamp_bus::Bus;
use lamp_msgs::{ExpressionBody, ExpressionLight, ExpressionState};
use std::{
    fs,
    io::Read,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

const PROGRAM: &str = env!("CARGO_BIN_EXE_lamp-expression");
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let path = PathBuf::from(format!(
            "/tmp/exp-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn serve(root: &Root) -> Running {
    Running(
        Command::new(PROGRAM)
            .args([
                "serve",
                "--clock-id",
                "test",
                "--seconds",
                "20",
                "--bus-root",
            ])
            .arg(&root.0)
            .env_remove("LAMP_BUS_ROOT")
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    )
}

fn state(bus: &Bus, child: &mut Running, previous_boot: Option<&str>) -> (ExpressionState, String) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(message) = bus.latest(ExpressionState::TOPIC).unwrap()
            && !message.expired
            && previous_boot != Some(message.header.boot.as_str())
            && let Ok(state) = message.parse()
        {
            return (state, message.header.boot);
        }
        if let Some(status) = child.0.try_wait().unwrap() {
            let mut error = String::new();
            if let Some(mut stderr) = child.0.stderr.take() {
                stderr.read_to_string(&mut error).unwrap();
            }
            panic!("service exited early ({status}): {error}");
        }
        assert!(Instant::now() < deadline, "service did not publish state");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn built_service_is_shadow_by_default_and_its_lock_survives_second_start() {
    let root = Root::new();
    let bus = Bus::new(&root.0).unwrap();
    let mut first = serve(&root);
    let (report, first_boot) = state(&bus, &mut first, None);
    assert!(!report.enabled && !report.body && !report.light);
    assert!(bus.latest(ExpressionBody::TOPIC).unwrap().is_none());
    assert!(bus.latest(ExpressionLight::TOPIC).unwrap().is_none());
    let second = Command::new(PROGRAM)
        .args(["serve", "--clock-id", "test", "--enable", "--bus-root"])
        .arg(&root.0)
        .output()
        .unwrap();
    assert!(!second.status.success());
    assert!(String::from_utf8_lossy(&second.stderr).contains("owner lock"));
    assert!(first.0.try_wait().unwrap().is_none());
    drop(first);
    // The existing lock file may be reopened after the owner is killed.
    let mut restarted = serve(&root);
    let (_, restarted_boot) = state(&bus, &mut restarted, Some(&first_boot));
    assert_ne!(first_boot, restarted_boot);
    assert!(restarted.0.try_wait().unwrap().is_none());
}

#[test]
fn a_catalogue_with_the_old_head_roll_still_serves_and_says_so_once() {
    // #955's own `speaking` section, as lamps were given it by hand.
    let mut catalogue: serde_json::Value =
        serde_json::from_str(include_str!("../behaviours.json")).unwrap();
    catalogue["speaking"] = serde_json::json!({
        "nod": {"base_yaw": 0.0, "shoulder": 0.0, "elbow": 0.0, "head_roll": 0.0, "head_pitch": 0.035},
        "nod_us": 360000,
        "nod_gap_us": 900000,
        "stress_level": 0.35,
        "stress_rise": 1.5,
        "sway_roll": 0.025,
        "pause_us": 350000,
        "sway_us": 600000,
        "settle": {"base_yaw": 0.0, "shoulder": 0.0, "elbow": 0.0, "head_roll": 0.0, "head_pitch": 0.03},
        "settle_us": 600000,
        "settle_after_us": 1500000
    });
    let root = Root::new();
    let file = root.0.join("expression.json");
    fs::write(&file, catalogue.to_string()).unwrap();
    let bus = Bus::new(&root.0).unwrap();
    let mut running = Running(
        Command::new(PROGRAM)
            .args(["serve", "--clock-id", "test", "--seconds", "20"])
            .arg("--catalogue")
            .arg(&file)
            .arg("--bus-root")
            .arg(&root.0)
            .env_remove("LAMP_BUS_ROOT")
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    state(&bus, &mut running, None);
    running.0.kill().unwrap();
    running.0.wait().unwrap();
    let mut error = String::new();
    running
        .0
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut error)
        .unwrap();
    assert_eq!(error.matches("sway_roll").count(), 1, "{error}");
}

#[test]
fn malformed_cli_and_existing_output_are_refused() {
    let help = Command::new(PROGRAM).arg("--help").output().unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("shadow"));
    let bad = Command::new(PROGRAM)
        .args(["serve", "--enable", "yes"])
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(2));
    let root = Root::new();
    let output = root.0.join("record.jsonl");
    fs::write(&output, b"keep this\n").unwrap();
    let refused = Command::new(PROGRAM)
        .args(["serve", "--clock-id", "test", "--bus-root"])
        .arg(&root.0)
        .arg("--record")
        .arg(&output)
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert_eq!(fs::read(&output).unwrap(), b"keep this\n");
    assert!(bus_not_started(&root));
}

fn bus_not_started(root: &Root) -> bool {
    !root.0.join(ExpressionState::TOPIC).exists()
}

#[test]
fn catalogue_and_clock_commands_need_no_bus_or_hardware() {
    let catalogue = Command::new(PROGRAM).arg("catalogue").output().unwrap();
    assert!(catalogue.status.success());
    lamp_expression::Catalogue::from_json(std::str::from_utf8(&catalogue.stdout).unwrap()).unwrap();
    let before = lamp_ipc::monotonic_us();
    let clock = Command::new(PROGRAM)
        .args(["clock", "--clock-id", "host_test"])
        .output()
        .unwrap();
    let after = lamp_ipc::monotonic_us();
    assert!(clock.status.success());
    let value: serde_json::Value = serde_json::from_slice(&clock.stdout).unwrap();
    assert_eq!(value["clock_id"], "host_test");
    let at = value["at_us"].as_u64().unwrap();
    assert!((before..=after).contains(&at));
}
