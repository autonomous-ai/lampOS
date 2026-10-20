use lamp_controls::touch_replay::{Frame, Summary};
use lamp_msgs::{ControlFault, TouchGesture, TouchIntent};
use serde_json::Value;
use std::{
    cell::Cell,
    fs::{self, File},
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

const PROGRAM: &str = env!("CARGO_BIN_EXE_lamp-controls");
const CLOCK: &str = "strip-711-authored";
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Root {
    path: PathBuf,
    command_id: Cell<u64>,
}

impl Root {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "touch-program-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        let root = Self {
            path,
            command_id: Cell::new(0),
        };
        root.restore();
        // Even a mistaken bus-open attempt cannot create a runtime directory.
        fs::write(root.file("bus-must-stay-file"), b"no bus in replay\n").unwrap();
        root
    }

    fn file(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }

    fn restore(&self) {
        let mut config: Value =
            serde_json::from_str(include_str!("../../fixtures/strip-711-controls.json")).unwrap();
        config["enabled"] = true.into();
        fs::write(
            self.file("controls.json"),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        fs::write(
            self.file("pins.json"),
            include_bytes!("../../fixtures/strip-711-pins-example.json"),
        )
        .unwrap();
        fs::write(
            self.file("input.jsonl"),
            include_bytes!("../../fixtures/strip-711-replay.jsonl"),
        )
        .unwrap();
    }

    fn command(&self) -> Command {
        let mut command = Command::new(PROGRAM);
        command.env("LAMP_BUS_ROOT", self.file("bus-must-stay-file"));
        command
    }

    fn replay_with_clock(&self, clock: &str) -> Command {
        let mut command = self.command();
        command
            .args(["replay", "--touch", "--clock-id", clock, "--config"])
            .arg(self.file("controls.json"))
            .arg("--pins")
            .arg(self.file("pins.json"))
            .arg("--input")
            .arg(self.file("input.jsonl"))
            .args(["--until-us", "19948000"]);
        command
    }

    fn replay(&self) -> Command {
        self.replay_with_clock(CLOCK)
    }

    /// This is a functional process bound, not a latency assertion. File
    /// redirection avoids blocked child pipes when replay emits many frames.
    fn run(&self, mut command: Command) -> Output {
        let id = self.command_id.get();
        self.command_id.set(id + 1);
        let stdout = self.file(&format!("stdout-{id}.jsonl"));
        let stderr = self.file(&format!("stderr-{id}.txt"));
        command
            .stdin(Stdio::null())
            .stdout(File::create(&stdout).unwrap())
            .stderr(File::create(&stderr).unwrap());
        let mut child = Running(Some(command.spawn().unwrap()));
        let end = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = child.0.as_mut().unwrap().try_wait().unwrap() {
                break status;
            }
            assert!(
                Instant::now() < end,
                "CLI exceeded 5 s; evidence {}",
                self.path.display()
            );
            thread::sleep(Duration::from_millis(5));
        };
        child.0.take().unwrap().wait().unwrap();
        assert_eq!(
            fs::read(self.file("bus-must-stay-file")).unwrap(),
            b"no bus in replay\n"
        );
        Output {
            status,
            stdout: fs::read(stdout).unwrap(),
            stderr: fs::read(stderr).unwrap(),
        }
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        if thread::panicking() {
            eprintln!("retained CLI failure evidence: {}", self.path.display());
        } else {
            fs::remove_dir_all(&self.path).unwrap();
        }
    }
}

struct Running(Option<Child>);
impl Drop for Running {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn successful(output: &Output) -> (Summary, Vec<Frame>) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let summary: Summary = serde_json::from_slice(&output.stderr).unwrap();
    let frames: Vec<Frame> = std::str::from_utf8(&output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    for frame in &frames {
        frame.output.state.check().unwrap();
        for event in &frame.output.events {
            event.check().unwrap();
        }
    }
    (summary, frames)
}

fn expected(summary: &Summary) {
    assert_eq!(
        (summary.records, summary.strip_samples, summary.gpio_samples),
        (1052, 1052, 0)
    );
    assert_eq!(
        (summary.rejected, summary.faults, summary.events),
        (0, 0, 54)
    );
    assert_eq!(
        (
            summary.ticks,
            summary.frames,
            summary.start_us,
            summary.end_us
        ),
        (9975, 1050, 0, 19_948_000)
    );
}

#[test]
fn real_cli_defaults_to_shadow_even_when_config_says_enabled_and_reports_exact_fixture_counts() {
    let root = Root::new();
    let output = root.run(root.replay());
    let (summary, frames) = successful(&output);
    expected(&summary);
    assert_eq!(frames.len(), 1050);
    assert!(frames.iter().all(|frame| !frame.output.state.enabled));
    let events: Vec<_> = frames
        .iter()
        .flat_map(|frame| &frame.output.events)
        .collect();
    assert!(
        events
            .iter()
            .all(|event| !event.enabled && !event.fresh_at(CLOCK, event.stamp.at_us))
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.gesture == TouchGesture::Slide)
            .count(),
        52
    );
    assert_eq!(events[0].when_asleep, Some(TouchIntent::Wake));
    let hold = events
        .iter()
        .find(|event| event.gesture == TouchGesture::Hold)
        .unwrap();
    assert_eq!(hold.when_quiet, TouchIntent::Sleep);
    assert_eq!(hold.when_asleep, Some(TouchIntent::Wake));
    eprintln!(
        "authored shadow CLI summary: {}",
        serde_json::to_string(&summary).unwrap()
    );
}

#[test]
fn enable_flag_changes_only_event_and_state_authority_fields() {
    let root = Root::new();
    let (shadow_summary, mut shadow) = successful(&root.run(root.replay()));
    let mut command = root.replay();
    command.arg("--enable");
    let (enabled_summary, enabled) = successful(&root.run(command));
    assert_eq!(shadow_summary, enabled_summary);
    for frame in &mut shadow {
        frame.output.state.enabled = true;
        for event in &mut frame.output.state.latest {
            event.enabled = true;
        }
        for event in &mut frame.output.events {
            event.enabled = true;
        }
    }
    assert_eq!(shadow, enabled);
}

#[test]
fn new_records_and_new_pins_schema_require_the_explicit_touch_selector() {
    let root = Root::new();
    for with_pins in [false, true] {
        let mut command = root.command();
        command
            .args(["replay", "--clock-id", CLOCK, "--config"])
            .arg(root.file("controls.json"))
            .arg("--input")
            .arg(root.file("input.jsonl"));
        if with_pins {
            command.arg("--pins").arg(root.file("pins.json"));
        }
        let output = root.run(command);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            if with_pins {
                error.contains("only with --touch")
            } else {
                error.contains("unknown field `input`")
            },
            "{error}"
        );
    }
}

#[test]
fn invalid_and_duplicate_cli_options_are_rejected_before_replay_or_output_creation() {
    let root = Root::new();
    for extra in [
        vec!["--touch"],
        vec!["--enable", "--enable"],
        vec!["--unknown"],
        vec!["--clock-id", CLOCK],
        vec!["--bus-root", "/tmp/unused"],
        vec!["--record", "unused.jsonl"],
        vec!["--until-us", "100"],
    ] {
        let mut command = root.replay();
        let path = root.file("must-not-create.jsonl");
        command.args(&extra).arg("--output").arg(&path);
        let output = root.run(command);
        assert!(!output.status.success(), "unexpected success: {extra:?}");
        assert!(output.stdout.is_empty());
        assert!(!path.exists());
    }
}

/// A recording's unknown field is refused before any output is made. The
/// files a newer build may have written, `controls.json` and the pins file,
/// are read past theirs, named once (#1680).
#[test]
fn an_unknown_input_field_is_refused_before_output_and_a_newer_files_field_is_named() {
    let root = Root::new();
    for target in ["controls.json", "pins.json"] {
        root.restore();
        let mut value: Value =
            serde_json::from_slice(&fs::read(root.file(target)).unwrap()).unwrap();
        value["unknown"] = true.into();
        fs::write(root.file(target), serde_json::to_vec(&value).unwrap()).unwrap();
        let path = root.file(&format!("read-past-{target}l"));
        let mut command = root.replay();
        command.arg("--output").arg(&path);
        let output = root.run(command);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stderr}");
        assert!(
            stderr.contains(&format!(
                "lamp-controls: {}: fields this build does not read, ignored: unknown",
                root.file(target).display()
            )),
            "{stderr}"
        );
        assert!(path.exists());
    }
    root.restore();
    fs::write(
        root.file("input.jsonl"),
        b"{\"now_us\":0,\"input\":{\"kind\":\"tick\",\"unknown\":true}}\n",
    )
    .unwrap();
    let path = root.file("uncreated.jsonl");
    let mut command = root.replay();
    command.arg("--output").arg(&path);
    let output = root.run(command);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown field"));
    assert!(!path.exists());
}

#[test]
fn output_is_private_jsonl_and_existing_evidence_is_never_overwritten() {
    let root = Root::new();
    let path = root.file("result.jsonl");
    let mut command = root.replay();
    command.arg("--output").arg(&path);
    let output = root.run(command);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    expected(&serde_json::from_slice(&output.stderr).unwrap());
    let original = fs::read(&path).unwrap();
    assert_eq!(
        std::str::from_utf8(&original).unwrap().lines().count(),
        1050
    );
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let stdout = root.run(root.replay());
    assert!(stdout.status.success());
    assert_eq!(original, stdout.stdout);
    assert_eq!(output.stderr, stdout.stderr);
    let mut retry = root.replay();
    retry.arg("--output").arg(&path);
    let refused = root.run(retry);
    assert!(!refused.status.success());
    assert_eq!(fs::read(&path).unwrap(), original);
}

#[test]
fn wrong_boot_clock_is_visible_in_summary_and_never_becomes_ready_or_actionable() {
    let root = Root::new();
    let (summary, frames) = successful(&root.run(root.replay_with_clock("another-authored-boot")));
    assert_eq!((summary.rejected, summary.events), (1052, 0));
    assert_eq!(summary.rejected_by_fault.len(), 1);
    assert_eq!(summary.rejected_by_fault[0].fault, ControlFault::WrongClock);
    assert!(
        frames
            .iter()
            .all(|frame| !frame.output.state.sources[0].ready)
    );
    assert!(
        frames
            .iter()
            .all(|frame| frame.output.state.sources[0].active.is_none())
    );
}

#[test]
fn replay_succeeds_with_a_nonexistent_i2c_adapter_and_an_unopenable_bus_root() {
    let root = Root::new();
    let absent = (0..=u8::MAX)
        .rev()
        .find(|bus| !Path::new(&format!("/dev/i2c-{bus}")).exists())
        .unwrap();
    let mut pins: Value =
        serde_json::from_slice(&fs::read(root.file("pins.json")).unwrap()).unwrap();
    pins["base"]["bus"] = absent.into();
    fs::write(root.file("pins.json"), serde_json::to_vec(&pins).unwrap()).unwrap();
    let (summary, _) = successful(&root.run(root.replay()));
    expected(&summary);
    // On non-Linux the hardware adapter itself is unsupported; successful
    // replay there independently proves it never entered the hardware path.
    assert!(root.file("bus-must-stay-file").is_file());
}

#[test]
fn working_pad_cli_profile_preserves_the_trace_with_individual_threshold_settings() {
    let root = Root::new();
    let mut baseline = root.replay();
    baseline.arg("--enable");
    let baseline = root.run(baseline);
    expected(&successful(&baseline).0);
    fs::write(
        root.file("controls.json"),
        include_bytes!("../../fixtures/strip-711-working-controls.json"),
    )
    .unwrap();
    fs::write(
        root.file("pins.json"),
        include_bytes!("../../fixtures/strip-711-working-pins-example.json"),
    )
    .unwrap();
    let mut command = root.replay();
    command.arg("--enable");
    let working = root.run(command);
    expected(&successful(&working).0);
    assert_eq!(working.stdout, baseline.stdout);
    assert_eq!(working.stderr, baseline.stderr);
}
