use lamp_bus::Bus;
use lamp_controls::{
    engine::Config,
    input::{Level as PinLevel, Observation},
    replay::{self, Frame, Record, Summary},
    runtime::{MissingPins, Service},
};
use lamp_msgs::{ClockStamp, ControlFault, ControlsState, Diagnostic, Level};
use std::{
    fs,
    path::PathBuf,
    process::{Child, Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

const PROGRAM: &str = env!("CARGO_BIN_EXE_lamp-controls");
const CLOCK: &str = "test-boot";
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let path = PathBuf::from(format!(
            "/tmp/cp-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn command(&self) -> Command {
        let mut command = Command::new(PROGRAM);
        // Even a broken dispatch must stay inside this test's temporary root.
        command.env("LAMP_BUS_ROOT", self.0.join("bus"));
        command
    }

    fn replay(&self) -> Command {
        let mut command = self.command();
        command
            .args(["replay", "--clock-id", CLOCK, "--config"])
            .arg(self.0.join("config.json"))
            .arg("--input")
            .arg(self.0.join("input.jsonl"));
        command
    }

    fn write_tap(&self) {
        let config = Config {
            enabled: true,
            buttons: vec!["primary".into()],
            ..Config::default()
        };
        fs::write(
            self.0.join("config.json"),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        let lines: Vec<_> = [(1_000_000, false), (1_010_000, true), (1_040_000, false)]
            .into_iter()
            .enumerate()
            .map(|(index, (at_us, active))| {
                serde_json::to_string(&Record {
                    now_us: at_us,
                    observation: Some(Observation {
                        stamp: ClockStamp {
                            clock_id: CLOCK.into(),
                            at_us,
                            sequence: index as u64 + 1,
                        },
                        session_id: "reader-one".into(),
                        buttons: vec![PinLevel {
                            name: "primary".into(),
                            active,
                        }],
                        head_pads: vec![],
                        base_mask: None,
                    }),
                    fault: None,

                    privacy: None,
                    privacy_fault: None,
                })
                .unwrap()
            })
            .collect();
        fs::write(self.0.join("input.jsonl"), lines.join("\n") + "\n").unwrap();
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn frames(output: &Output) -> Vec<Frame> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn replay_ignores_config_enable_without_the_explicit_flag() {
    let root = Root::new();
    root.write_tap();
    let shadow = root.replay().output().unwrap();
    let shadow_frames = frames(&shadow);
    assert!(!shadow_frames.is_empty());
    assert!(
        shadow_frames
            .iter()
            .all(|frame| !frame.output.state.enabled)
    );
    let gestures: Vec<_> = shadow_frames
        .iter()
        .flat_map(|frame| &frame.output.gestures)
        .collect();
    assert_eq!(gestures.len(), 1);
    assert!(!gestures[0].enabled);
    let summary: Summary = serde_json::from_slice(&shadow.stderr).unwrap();
    assert_eq!(summary.gestures, 1);
    assert_eq!(summary.rejected, 0);

    let enabled = root.replay().arg("--enable").output().unwrap();
    let enabled_frames = frames(&enabled);
    assert!(
        enabled_frames
            .iter()
            .all(|frame| frame.output.state.enabled)
    );
    let enabled_gestures: Vec<_> = enabled_frames
        .iter()
        .flat_map(|frame| &frame.output.gestures)
        .collect();
    assert_eq!(enabled_gestures.len(), 1);
    assert!(enabled_gestures[0].enabled);
    assert_eq!(gestures[0].duration_us, enabled_gestures[0].duration_us);
    assert!(
        !root.0.join("bus").exists(),
        "replay must not open a runtime bus"
    );
}

#[test]
fn replay_output_is_jsonl_and_an_existing_file_is_never_overwritten() {
    let root = Root::new();
    root.write_tap();
    let path = root.0.join("output.jsonl");
    let output = root.replay().arg("--output").arg(&path).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    let original = fs::read(&path).unwrap();
    assert!(!original.is_empty());
    for line in String::from_utf8(original.clone()).unwrap().lines() {
        let _: Frame = serde_json::from_str(line).unwrap();
    }
    let refused = root.replay().arg("--output").arg(&path).output().unwrap();
    assert!(!refused.status.success());
    assert_eq!(fs::read(path).unwrap(), original);
}

#[test]
fn invalid_arguments_fail_before_any_bus_or_record_is_created() {
    let root = Root::new();
    for args in [
        vec!["serve", "--unknown"],
        vec!["serve", "--enable", "--enable"],
        vec!["serve", "--bus-root", "relative"],
        vec!["serve", "--seconds", "0"],
        vec!["serve", "--seconds", "86401"],
        vec!["serve", "--clock-id"],
        vec!["replay", "--clock-id", CLOCK],
        vec!["clock", "--enable"],
        vec!["clock", "--clock-id", "../boot"],
        vec!["clock", "--clock-id", "two words"],
    ] {
        let result = root.command().args(&args).output().unwrap();
        assert!(!result.status.success(), "unexpected success for {args:?}");
        assert!(result.stdout.is_empty(), "unexpected output for {args:?}");
        assert!(!result.stderr.is_empty());
    }
    assert!(!root.0.join("bus").exists());
}

#[test]
fn invalid_config_or_replay_is_rejected_before_creating_output() {
    let root = Root::new();
    root.write_tap();
    let output = root.0.join("uncreated.jsonl");
    fs::write(
        root.0.join("config.json"),
        b"{\"enabled\":true,\"unexpected\":1}",
    )
    .unwrap();
    assert!(
        !root
            .replay()
            .arg("--output")
            .arg(&output)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(!output.exists());
    root.write_tap();
    fs::write(root.0.join("input.jsonl"), b"{broken json}\n").unwrap();
    assert!(
        !root
            .replay()
            .arg("--output")
            .arg(&output)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(!output.exists());
    root.write_tap();
    fs::write(root.0.join("config.json"), vec![b' '; 16_385]).unwrap();
    let oversized = root.replay().output().unwrap();
    assert!(!oversized.status.success());
    assert!(String::from_utf8_lossy(&oversized.stderr).contains("16 KiB"));
}

#[test]
fn existing_acquisition_record_is_refused_without_truncation() {
    let root = Root::new();
    let record = root.0.join("keep.jsonl");
    fs::write(&record, b"existing evidence\n").unwrap();
    let result = root
        .command()
        .args(["serve", "--clock-id", CLOCK, "--seconds", "1", "--record"])
        .arg(&record)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(fs::read(record).unwrap(), b"existing evidence\n");
    // A failed startup also releases the owner lock, so a corrected restart
    // is possible without deleting or replacing the lock file.
    let bus = Bus::new(root.0.join("bus")).unwrap();
    assert!(Service::new(&bus, Config::default(), MissingPins, CLOCK).is_ok());
}

#[test]
fn clock_command_reports_the_requested_identity_and_a_monotonic_value() {
    let root = Root::new();
    let mut times = Vec::new();
    for _ in 0..2 {
        let output = root
            .command()
            .args(["clock", "--clock-id", CLOCK])
            .output()
            .unwrap();
        assert!(output.status.success());
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["clock_id"], CLOCK);
        times.push(value["at_us"].as_u64().unwrap());
    }
    assert!(times[1] >= times[0]);
    assert!(!root.0.join("bus").exists());
}

struct Running(Option<Child>);
impl Running {
    fn finish(mut self) -> Output {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if self.0.as_mut().unwrap().try_wait().unwrap().is_some() {
                return self.0.take().unwrap().wait_with_output().unwrap();
            }
            assert!(
                Instant::now() < deadline,
                "serve did not exit after SIGTERM"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[test]
fn sigterm_finishes_recording_and_releases_the_actual_serve_owner() {
    let root = Root::new();
    let bus = Bus::new(root.0.join("bus")).unwrap();
    let mut reports = bus
        .subscribe_typed(Diagnostic::TOPIC, "test", Diagnostic::TYPE)
        .unwrap();
    let record = root.0.join("acquisitions.jsonl");
    let child = root
        .command()
        .args(["serve", "--clock-id", CLOCK, "--enable", "--record"])
        .arg(&record)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let running = Running(Some(child));
    // Waiting on the real bus proves startup and signal registration, without
    // guessing a startup sleep or enabling a hardware adapter.
    let report: Diagnostic = reports
        .recv(Duration::from_secs(3))
        .unwrap()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(report.level, Level::Error);
    let before: ControlsState = bus
        .latest(ControlsState::TOPIC)
        .unwrap()
        .unwrap()
        .parse()
        .unwrap();
    assert!(before.enabled && !before.ready);
    assert_eq!(before.fault, Some(ControlFault::Missing));
    assert!(Service::new(&bus, Config::default(), MissingPins, CLOCK).is_err());

    // Sent from here, not by running `/bin/kill`: a machine without procps
    // (the ARM64 cross image these tests run in under qemu) has none, and
    // the test failed there before any signal (the first builder on #705).
    let pid = running.0.as_ref().unwrap().id();
    rustix::process::kill_process(
        rustix::process::Pid::from_raw(pid as i32).expect("a child's pid"),
        rustix::process::Signal::TERM,
    )
    .unwrap();
    let output = running.finish();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let summary: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(summary["physical_stop_verified"], false);
    assert!(summary["counts"]["reader_errors"].as_u64().unwrap() > 0);
    assert_eq!(summary["recording"]["failed"], false);
    assert_eq!(summary["recording"]["finished"], true);
    assert_eq!(summary["recording"]["dropped"], 0);
    let records = replay::read(fs::File::open(record).unwrap()).unwrap();
    assert!(!records.is_empty());
    assert!(
        records
            .iter()
            .all(|row| row.observation.is_none() && row.fault == Some(ControlFault::Missing))
    );
    let mut restarted = Service::new(&bus, Config::default(), MissingPins, CLOCK).unwrap();
    let after = restarted.step(lamp_ipc::monotonic_us);
    assert_ne!(after.state.session_id, before.session_id);
    assert!(!after.state.ready);
}
