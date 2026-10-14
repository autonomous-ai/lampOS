//! Real owned subprocesses and Unix channels, with the unchanged motor core
//! behind an inert protocol emulator. No serial/GPIO/camera/audio device opens.
use super::*;
use lamp_body::{
    control::{self, BusPort, MotorControl},
    runtime::{
        SystemClock,
        channel::Channels,
        config::*,
        supervisor::parse_boot,
        wire,
        worker::{self, Backend, Link},
    },
    units::NormalizedPosition,
};
use lamp_turn_owner::{AdmissionState, AdmittedInput, CaptureState, Controller};
use std::{
    cell::Cell,
    collections::VecDeque,
    os::unix::fs::PermissionsExt,
    rc::Rc,
    time::{Duration, Instant},
};

fn config() -> Config {
    Config {
        device: "/dev/not-a-device-inert-test".into(),
        lock_directory: "/not-opened/lock".into(),
        expected_device: lamp_body::runtime::config::Identity {
            filesystem_device: 1,
            inode: 2,
            character_device: 3,
            major: 4,
            minor: 5,
        },
        unit: "inert".into(),
        calibration: std::array::from_fn(|i| Calibration {
            joint: i as u8 + 1,
            id: i as u16 + 1,
            drive_mode: 0,
            homing_offset: 0,
            range_min: 1000,
            range_max: 3000,
        }),
        register_binding: std::array::from_fn(|i| RegisterBinding {
            joint: i as u8 + 1,
            minimum: 1000,
            maximum: 3000,
            homing_offset: 0,
        }),
        qualification: Qualification {
            selected_profiles: None,
            record: [7; 32],
            valid_from_us: monotonic_us().saturating_sub(1_000),
            expires_at_us: monotonic_us() + 10_000_000,
            joints: [Envelope {
                minimum: -70.0,
                maximum: 70.0,
                velocity: 1000.0,
                acceleration: 50_000.0,
                raw_acceleration: 10,
                raw_goal_time: 0,
                raw_speed: 500,
                maximum_load_raw: 100,
                maximum_velocity_raw: 1000,
                tracking_error: 30.0,
                settling_error: 0.5,
            }; 5],
        },
        timing: Bounds {
            transaction_us: 40_000,
            io_us: 5_000,
            readback_age_us: 200_000,
            placement_age_us: 500_000,
            interval_us: 10_000,
        },
        placement_source: BootId::new([3; 16]).unwrap(),
        maximum_runtime_us: 3_000_000,
    }
}
struct Port {
    rx: VecDeque<u8>,
    positions: [u16; 5],
    writes: usize,
    position_writes: Rc<Cell<usize>>,
    last_positions: Rc<Cell<[u16; 5]>>,
    emitted_mask: Rc<Cell<u8>>,
    fail_discard: bool,
}
impl BusPort for Port {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.rx.is_empty() {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let n = out.len().min(self.rx.len());
        for value in &mut out[..n] {
            *value = self.rx.pop_front().unwrap();
        }
        Ok(n)
    }
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.writes += 1;
        assert_eq!(data.len(), data[3] as usize + 4);
        assert_eq!(
            *data.last().unwrap(),
            !data[2..data.len() - 1]
                .iter()
                .fold(0u8, |a, b| a.wrapping_add(*b))
        );
        match data[4] {
            0x82 => {
                for id in 1..=5 {
                    let payload = match (data[5], data[6]) {
                        (3, 2) => 777u16.to_le_bytes().to_vec(),
                        (9, 4) => [1000u16.to_le_bytes(), 3000u16.to_le_bytes()].concat(),
                        (31, 3) => vec![0, 0, 0],
                        (40, 8) => {
                            let goal = self.positions[id as usize - 1].to_le_bytes();
                            vec![1, 10, goal[0], goal[1], 0, 0, 244, 1]
                        }
                        (56, 15) => {
                            let mut p = vec![0; 15];
                            p[..2].copy_from_slice(&self.positions[id as usize - 1].to_le_bytes());
                            p
                        }
                        other => panic!("unexpected read: {other:?}"),
                    };
                    let mut packet = vec![255, 255, id, payload.len() as u8 + 2, 0];
                    packet.extend(payload);
                    packet.push(!packet[2..].iter().fold(0u8, |a, b| a.wrapping_add(*b)));
                    self.rx.extend(packet);
                }
            }
            0x03 => {
                assert_eq!(data[2], 5);
                match (data[5], data.len()) {
                    (46, 9) => assert_eq!(&data[6..8], &500u16.to_le_bytes()),
                    (41, 8) => assert_eq!(data[6], 10),
                    other => panic!("forbidden setup write: {other:?}"),
                }
                self.rx.extend([255, 255, 5, 2, 0, 248]);
            }
            0x83 => {
                self.position_writes.set(self.position_writes.get() + 1);
                assert_eq!(&data[5..7], &[42, 2]);
                let mut previous = 0;
                for entry in data[7..data.len() - 1].chunks_exact(3) {
                    assert!(entry[0] > previous && entry[0] <= 5);
                    previous = entry[0];
                    self.emitted_mask
                        .set(self.emitted_mask.get() | (1 << (entry[0] - 1)));
                    self.positions[usize::from(entry[0] - 1)] =
                        u16::from_le_bytes([entry[1], entry[2]]);
                }
            }
            value => panic!("forbidden motor instruction {value}"),
        }
        self.last_positions.set(self.positions);
        Ok(data.len())
    }
    fn discard_output(&mut self) -> io::Result<()> {
        if self.fail_discard {
            Err(io::Error::other("inert discard failed"))
        } else {
            Ok(())
        }
    }
}
struct InertBackend {
    core: MotorControl<Port, SystemClock>,
    bad_cleanup: bool,
    receipt: PathBuf,
    position_writes: Rc<Cell<usize>>,
    last_positions: Rc<Cell<[u16; 5]>>,
    emitted_mask: Rc<Cell<u8>>,
}
impl Backend for InertBackend {
    fn install(&mut self, s: Snapshot) -> Result<(), control::Error> {
        self.core.install(s)
    }
    fn inspect(&mut self) -> Result<(), control::Error> {
        self.core.inspect()
    }
    fn start(
        &mut self,
        p: OutputPermit,
        o: control::Placement,
        t: [NormalizedPosition; 5],
        d: u64,
    ) -> Result<control::MotionId, control::Error> {
        self.core.start_segment(p, o, t, d)
    }
    fn start_selected(
        &mut self,
        p: OutputPermit,
        o: control::Placement,
        t: [NormalizedPosition; 5],
        d: u64,
        selection: JointSelection,
    ) -> Result<control::MotionId, control::Error> {
        self.core.start_selected_segment(p, o, t, d, selection)
    }
    fn prepare_profile(
        &mut self,
        permit: OutputPermit,
        placement: control::Placement,
        speed: u16,
        acceleration: u8,
    ) -> Result<control::MotionId, control::Error> {
        self.core
            .prepare_head_profile(permit, placement, speed, acceleration)
    }
    fn readback(&mut self) -> Result<control::Readback, control::Error> {
        self.core.readback()
    }
    fn tick(&mut self) -> Result<control::Tick, control::Error> {
        self.core.tick()
    }
    fn cancel(&mut self, m: control::MotionId) -> Result<control::StopReport, control::Error> {
        self.core.cancel(m)
    }
    fn inhibit(&mut self, normal: bool) -> control::StopReport {
        if normal {
            self.core.shutdown()
        } else {
            self.core.control_lost()
        }
    }
    fn stopped(&self) -> Option<control::StopReport> {
        self.core.stop_report()
    }
    fn finish(mut self) -> wire::Cleanup {
        let report = self.core.shutdown();
        fs::write(&self.receipt, format!("{:?}", report.reason)).unwrap();
        fs::write(
            self.receipt.with_file_name("child-position-writes"),
            self.position_writes.get().to_string(),
        )
        .unwrap();
        fs::write(
            self.receipt.with_file_name("child-final-counts"),
            serde_json::to_vec(&self.last_positions.get()).unwrap(),
        )
        .unwrap();
        fs::write(
            self.receipt.with_file_name("child-emitted-mask"),
            self.emitted_mask.get().to_string(),
        )
        .unwrap();
        // Synthetic resource-close receipts for a fake port, never hardware proof.
        wire::Cleanup {
            serial_closed: Some(!self.bad_cleanup),
            settings_restored: Some(true),
            error: self.bad_cleanup.then(|| "inert close failed".into()),
        }
    }
}
struct TestLink {
    channels: Channels,
    mode: String,
    directory: PathBuf,
}
impl Link for TestLink {
    fn control(&mut self, n: MonoTime) -> io::Result<Option<Control>> {
        self.channels.control.receive(n)
    }
    fn command(&mut self, n: MonoTime) -> io::Result<Option<Command>> {
        self.channels.data.receive(n)
    }
    fn publish(&mut self, e: &Event, n: MonoTime) -> io::Result<()> {
        if matches!(e, Event::Ready { .. }) {
            eprintln!("motor_fixture_phase=ready_send at_us={}", monotonic_us());
        }
        self.channels.control.send(e, n)?;
        if matches!(e, Event::Stopped { .. }) && self.mode == "gated_exit_failure" {
            let until = Instant::now() + Duration::from_secs(2);
            while !self.directory.join("go").exists() {
                assert!(Instant::now() < until);
                std::thread::park_timeout(Duration::from_millis(1));
            }
            std::process::exit(7);
        }
        if matches!(e, Event::Inspected { .. }) && self.mode == "gate_segment" {
            let until = Instant::now() + Duration::from_secs(2);
            while !self.directory.join("go").exists() {
                assert!(Instant::now() < until);
                std::thread::park_timeout(Duration::from_millis(1));
            }
        }
        if matches!(e, Event::Ready { .. }) {
            match self.mode.as_str() {
                "crash" => std::process::exit(7),
                "hang" => loop {
                    std::thread::park_timeout(Duration::from_secs(1));
                },
                "gate" => {
                    let until = Instant::now() + Duration::from_secs(2);
                    while !self.directory.join("go").exists() {
                        assert!(Instant::now() < until);
                        std::thread::park_timeout(Duration::from_millis(1));
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}
#[test]
fn inert_child() {
    let Ok(directory) = std::env::var("LAMP_LIVE_MOTOR_TEST_DIRECTORY") else {
        return;
    };
    eprintln!("motor_fixture_phase=child_entry at_us={}", monotonic_us());
    let directory = PathBuf::from(directory);
    let mode = std::env::var("LAMP_LIVE_MOTOR_TEST_MODE").unwrap();
    let parent = parse_boot(&std::env::var("LAMP_LIVE_MOTOR_TEST_PARENT").unwrap()).unwrap();
    let own = parse_boot(&std::env::var("LAMP_LIVE_MOTOR_TEST_WORKER").unwrap()).unwrap();
    let c = Config::read(Path::new(
        &std::env::var("LAMP_LIVE_MOTOR_TEST_CONFIG").unwrap(),
    ))
    .unwrap();
    let valid = c.validate().unwrap();
    let mut channels = Channels::bind(&directory, parent, own, true).unwrap();
    eprintln!(
        "motor_fixture_phase=channels_bound at_us={}",
        monotonic_us()
    );
    let until = Instant::now() + Duration::from_secs(2);
    while !channels.connect_step().unwrap() {
        assert!(Instant::now() < until);
        std::thread::park_timeout(Duration::from_millis(1));
    }
    eprintln!(
        "motor_fixture_phase=channels_connected at_us={}",
        monotonic_us()
    );
    let position_writes = Rc::new(Cell::new(0));
    let last_positions = Rc::new(Cell::new([2000; 5]));
    let emitted_mask = Rc::new(Cell::new(0));
    let mut core = MotorControl::new(
        Port {
            rx: VecDeque::new(),
            positions: [2000; 5],
            writes: 0,
            position_writes: position_writes.clone(),
            last_positions: last_positions.clone(),
            emitted_mask: emitted_mask.clone(),
            fail_discard: mode == "discard",
        },
        SystemClock,
        parent,
        own,
        &valid.unit,
        valid.calibration,
        valid.timing,
    )
    .unwrap();
    core.qualify(valid.qualification).unwrap();
    let backend = InertBackend {
        core,
        bad_cleanup: mode == "bad_cleanup",
        receipt: directory.parent().unwrap().join("child-finish"),
        position_writes,
        last_positions,
        emitted_mask,
    };
    let worker = worker::Worker::new(
        backend,
        SystemClock,
        parent,
        own,
        c.placement_source,
        c.maximum_runtime_us,
    )
    .unwrap();
    let mut link = TestLink {
        channels,
        mode,
        directory,
    };
    let result = worker::run(worker, &mut link);
    eprintln!(
        "motor_fixture_phase=worker_finished result={result:?} at_us={}",
        monotonic_us()
    );
    if result.is_err() {
        std::process::exit(3);
    }
}
fn quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
fn fixture(mode: &str) -> (crate::process::SessionDirectory, OptionalMotor, Controller) {
    let (dir, prepared) = prepare_fixture(mode);
    let mut controller = Controller::new(new_boot().unwrap(), time());
    controller
        .set_microphone_permission(time(), Permission::Allowed)
        .unwrap();
    controller
        .set_camera_permission(time(), Permission::Allowed)
        .unwrap();
    let mut motor = OptionalMotor::new(controller.snapshot(time()).unwrap().boot(), Some(prepared));
    if mode != "startup" {
        motor.start_before_capture(controller.snapshot(time()).unwrap(), false, true);
    }
    (dir, motor, controller)
}
fn prepare_fixture(mode: &str) -> (crate::process::SessionDirectory, PreparedMotor) {
    let dir = crate::process::SessionDirectory::create().unwrap();
    let mut c = config();
    if mode == "profile" {
        c.qualification.selected_profiles = Some(SelectedProfiles {
            selection: JointSelection::from_mask(16).unwrap(),
            prepare_from: Some(
                [ObservedProfile {
                    acceleration: 10,
                    goal_time: 0,
                    speed: 500,
                }; 5],
            ),
        });
    }
    let path = dir.path.join("input.json");
    fs::write(&path, serde_json::to_vec(&c).unwrap()).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let executable = dir.path.join("test-worker");
    let script = format!(
        "#!/bin/sh\nif [ \"$1\" = --fixture-preflight ]; then printf R; exit 0; fi\nexport LAMP_LIVE_MOTOR_TEST_CONFIG=\"$2\" LAMP_LIVE_MOTOR_TEST_DIRECTORY=\"$4\" LAMP_LIVE_MOTOR_TEST_PARENT=\"$6\" LAMP_LIVE_MOTOR_TEST_WORKER=\"$8\"\nexport LAMP_LIVE_MOTOR_TEST_MODE={}\nexec {} --exact coordinator::motor::tests::inert_child --nocapture >\"$LAMP_LIVE_MOTOR_TEST_DIRECTORY/child-output\" 2>&1\n",
        quoted(mode),
        quoted(std::env::current_exe().unwrap().to_str().unwrap())
    );
    fs::write(&executable, script).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let prepared = PreparedMotor::load(
        &MotorOptions {
            head: None,
            worker: executable,
            config: path,
        },
        &dir.path,
    )
    .unwrap();
    // A fresh executable script has a substantial first-exec cost on the Intel
    // Mac development host. Complete that fixture-only setup using the exact
    // frozen inode before timing the real supervisor/worker protocol. This arm
    // exits before exporting worker arguments or binding/opening any resource.
    // It does not establish motor Ready, and no supervisor deadline changes.
    preflight(
        &prepared.executable.path,
        crate::process::WORKER_STARTUP_TIMEOUT,
    )
    .unwrap();
    assert!(!prepared.directory.exists());
    (dir, prepared)
}

fn preflight(executable: &Path, timeout: Duration) -> io::Result<()> {
    use std::process::{Command as ProcessCommand, Stdio};

    let until = Instant::now() + timeout;
    let mut child = ProcessCommand::new(executable)
        .arg("--fixture-preflight")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let result = (|| {
        let mut output = child.stdout.take().expect("piped preflight marker");
        let flags = rustix::fs::fcntl_getfl(&output)?;
        rustix::fs::fcntl_setfl(&output, flags | rustix::fs::OFlags::NONBLOCK)?;
        let mut bytes = [0; 2];
        let mut received = 0;
        loop {
            match output.read(&mut bytes[received..]) {
                Ok(n) => received += n,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(error),
            }
            if let Some(status) = child.try_wait()? {
                // Exit can race the first nonblocking read. Once reaped, drain
                // the final marker before judging the completed preflight.
                match output.read(&mut bytes[received..]) {
                    Ok(n) => received += n,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                    Err(error) => return Err(error),
                }
                if !status.success() || received != 1 {
                    return Err(invalid(format!(
                        "motor fixture preflight exited without success/marker: {status}"
                    )));
                }
                if bytes[0] != b'R' {
                    return Err(invalid("unexpected motor fixture preflight marker"));
                }
                return Ok(());
            }
            if received > 1 || (received == 1 && bytes[0] != b'R') {
                return Err(invalid("unexpected motor fixture preflight marker"));
            }
            if Instant::now() >= until {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "motor fixture preflight did not exit",
                ));
            }
            std::thread::park_timeout(Duration::from_millis(1));
        }
    })();
    if result.is_err() {
        // The fixture owns this one direct child. Never leave its wait status
        // or stdout handle to a later test, including marker/timeout failures.
        let _ = child.kill();
        child
            .wait()
            .map_err(|error| invalid(format!("motor fixture preflight cleanup failed: {error}")))?;
    }
    result
}

#[test]
fn frozen_fixture_preflight_is_not_motor_startup() {
    let (dir, prepared) = prepare_fixture("normal");
    assert!(!prepared.directory.exists());
    let meta = fs::metadata(&prepared.executable.path).unwrap();
    assert_eq!(meta.dev(), prepared.executable.device);
    assert_eq!(meta.ino(), prepared.executable.inode);
    assert_eq!(
        format!(
            "{:x}",
            Sha256::digest(fs::read(&prepared.executable.path).unwrap())
        ),
        prepared.identity.executable_sha256
    );
    drop(prepared);
    fs::remove_dir_all(&dir.path).unwrap();
}

#[test]
fn fixture_preflight_rejects_missing_wrong_and_failed_markers_and_reaps() {
    for body in ["exit 0", "printf X; exit 0", "printf R; exit 7"] {
        let dir = crate::process::SessionDirectory::create().unwrap();
        let script = dir.path.join("preflight");
        let pid_file = dir.path.join("pid");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf '%s' \"$$\" >{}\n{body}\n",
                quoted(pid_file.to_str().unwrap())
            ),
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(preflight(&script, crate::process::WORKER_STARTUP_TIMEOUT).is_err());
        assert_preflight_reaped(&pid_file);
        fs::remove_dir_all(&dir.path).unwrap();
    }
}

#[test]
fn fixture_preflight_marker_without_exit_times_out_and_reaps() {
    let dir = crate::process::SessionDirectory::create().unwrap();
    let script = dir.path.join("preflight");
    let pid_file = dir.path.join("pid");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '%s' \"$$\" >{}\nprintf R\nexec sleep 60\n",
            quoted(pid_file.to_str().unwrap())
        ),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    let error = preflight(&script, crate::process::WORKER_STARTUP_TIMEOUT).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert_preflight_reaped(&pid_file);
    fs::remove_dir_all(&dir.path).unwrap();
}

fn assert_preflight_reaped(pid_file: &Path) {
    let pid = fs::read_to_string(pid_file).unwrap().parse().unwrap();
    let pid = rustix::process::Pid::from_raw(pid).unwrap();
    assert_eq!(
        rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::NOHANG).unwrap_err(),
        rustix::io::Errno::CHILD,
        "the exact preflight child must already have been reaped"
    );
}

fn wait(
    motor: &mut OptionalMotor,
    c: &mut Controller,
    mut predicate: impl FnMut(&OptionalMotor) -> bool,
) {
    let until = Instant::now() + Duration::from_secs(2);
    let mut last = 0;
    let mut last_poll = Instant::now();
    let mut largest_poll_gap = Duration::ZERO;
    while !predicate(motor) {
        largest_poll_gap = largest_poll_gap.max(last_poll.elapsed());
        assert!(
            Instant::now() < until,
            "owned child timed out: adapter={:?}; connected={}; ready={}; reaped={}; supervisor={:?}; largest_poll_gap_us={}; output={}",
            motor.failure(),
            motor.child.as_ref().is_some_and(Supervisor::connected),
            motor.is_ready(),
            motor.reaped(),
            motor.child.as_ref().and_then(Supervisor::failure),
            largest_poll_gap.as_micros(),
            motor
                .prepared
                .as_ref()
                .and_then(|prepared| File::open(prepared.directory.join("child-output")).ok())
                .map(|file| {
                    let mut bytes = Vec::new();
                    file.take(2048).read_to_end(&mut bytes).unwrap();
                    String::from_utf8_lossy(&bytes).into_owned()
                })
                .unwrap_or_default()
        );
        if monotonic_us() - last >= 10_000 {
            motor.publish(c.snapshot(time()).unwrap());
            last = monotonic_us();
        }
        motor.poll(|_| {});
        last_poll = Instant::now();
        std::thread::park_timeout(Duration::from_millis(1));
    }
    // Read terminal packets that raced the observed child exit.
    for _ in 0..2 {
        motor.poll(|_| {});
    }
}
fn cleanup(
    mut motor: OptionalMotor,
    mut controller: Controller,
    dir: crate::process::SessionDirectory,
) {
    motor.begin_shutdown();
    wait(&mut motor, &mut controller, OptionalMotor::reaped);
    drop(motor);
    fs::remove_dir_all(&dir.path).unwrap();
}
fn evidence(motor: &mut OptionalMotor, c: &mut Controller, request: u64) -> Evidence {
    let at = time();
    let until = at.checked_add(900_000).unwrap();
    c.set_capture(at, CaptureState::RetainingUntil(until))
        .unwrap();
    c.set_admission(at, AdmissionState::OpenUntil(until))
        .unwrap();
    let owner = if let Some(owner) = c.snapshot(at).unwrap().owner() {
        owner
    } else {
        c.admit(at, AdmittedInput::NewTurn).unwrap()
    };
    let camera = c.camera_grant(at).unwrap();
    let plan = c
        .plan_output(at, owner, OutputKind::Motion, Some(camera))
        .unwrap();
    let permit = c.issue_output(at, plan, 240_000).unwrap();
    motor.publish(c.snapshot(time()).unwrap());
    Evidence {
        request,
        requested_at_us: at.as_micros(),
        permit,
        placement: Observation {
            source_boot: BootId::new([3; 16]).unwrap(),
            frame_sequence: request,
            acquired_at_us: at.as_micros(),
            valid_until_us: at.as_micros() + 500_000,
            camera,
            qualification_record: [7; 32],
        },
    }
}
#[test]
fn idle_ready_never_inspects_moves_or_claims_listening() {
    let (dir, mut m, mut c) = fixture("normal");
    wait(&mut m, &mut c, OptionalMotor::is_ready);
    assert_eq!(m.offers, 0);
    assert!(m.inspected_at_us.is_none());
    assert!(!c.snapshot(time()).unwrap().listening_ready(time()));
    m.begin_shutdown();
    wait(&mut m, &mut c, OptionalMotor::reaped);
    assert!(m.finish().is_ok());
    assert_eq!(
        fs::read_to_string(dir.path.join("child-finish")).unwrap(),
        "Shutdown"
    );
    cleanup(m, c, dir);
}
#[test]
fn configured_camera_or_original_permissions_are_required_before_spawn() {
    for (capture, camera, denied) in [
        (true, true, false),
        (false, false, false),
        (false, true, true),
    ] {
        let (dir, mut m, mut c) = fixture("normal");
        m.begin_shutdown();
        wait(&mut m, &mut c, OptionalMotor::reaped);
        let prepared = m.prepared.take();
        drop(m);
        let mut candidate = OptionalMotor::new(c.snapshot(time()).unwrap().boot(), prepared);
        if denied {
            c.set_camera_permission(time(), Permission::Denied).unwrap();
        }
        candidate.start_before_capture(c.snapshot(time()).unwrap(), capture, camera);
        assert!(!candidate.started());
        assert!(!candidate.is_ready());
        assert!(candidate.failure().is_some());
        cleanup(candidate, c, dir);
    }
}
#[test]
fn readback_and_original_placement_are_required_and_bad_offers_do_not_enqueue() {
    let (dir, mut m, mut c) = fixture("normal");
    wait(&mut m, &mut c, OptionalMotor::is_ready);
    let e = evidence(&mut m, &mut c, 1);
    let mut offer = MovementOffer {
        selection: JointSelection::ALL,
        evidence: e,
        target: [0.0; 5],
        duration_us: 40_000,
    };
    assert!(
        m.offer_segment(offer)
            .unwrap_err()
            .to_string()
            .contains("readback")
    );
    offer.evidence.placement.source_boot = new_boot().unwrap();
    assert!(m.offer_segment(offer).is_err());
    offer.evidence = e;
    offer.evidence.requested_at_us = monotonic_us() - 100_001;
    assert!(m.offer_segment(offer).is_err());
    assert_eq!(m.offers, 0);
    m.offer_inspection(e).unwrap();
    wait(&mut m, &mut c, |m| m.inspected_at_us.is_some());
    let e = evidence(&mut m, &mut c, 2);
    m.offer_segment(MovementOffer {
        selection: JointSelection::ALL,
        evidence: e,
        target: [0.0; 5],
        duration_us: 40_000,
    })
    .unwrap();
    wait(&mut m, &mut c, |m| m.active.is_some());
    let active = m.active.unwrap().motion;
    let stale = Motion {
        sequence: active.sequence + 1,
        ..active
    };
    assert!(m.cancel(stale).is_err());
    assert!(m.is_ready());
    let cancel_at = Instant::now();
    m.cancel(active).unwrap();
    eprintln!(
        "motor_cancel_priority_send_us={}",
        cancel_at.elapsed().as_micros()
    );
    wait(&mut m, &mut c, OptionalMotor::reaped);
    assert!(m.finish().is_ok());
    cleanup(m, c, dir);
}
#[test]
fn pending_inspection_is_revoked_before_worker_data_and_release_is_separate() {
    let (dir, mut m, mut c) = fixture("gate");
    wait(&mut m, &mut c, OptionalMotor::is_ready);
    let e = evidence(&mut m, &mut c, 1);
    m.offer_inspection(e).unwrap();
    c.set_microphone_permission(time(), Permission::Denied)
        .unwrap();
    let denied = c.snapshot(time()).unwrap();
    let cancelled_at = Instant::now();
    m.publish(denied);
    eprintln!(
        "motor_local_revocation_us={}",
        cancelled_at.elapsed().as_micros()
    );
    assert!(!m.is_ready());
    assert!(m.pending.is_none());
    fs::write(dir.path.join("motor/go"), b"release inert child").unwrap();
    wait(&mut m, &mut c, OptionalMotor::reaped);
    let child = m.child.as_ref().unwrap();
    assert!(!child.clean_exit());
    assert!(child.privacy_release_confirmed(denied.revision()));
    assert!(!child.privacy_release_confirmed(denied.revision() + 1));
    assert!(m.finish().is_ok());
    assert!(m.may_rearm_after_cycle(true));
    assert!(!m.may_rearm_after_cycle(false));
    assert_eq!(
        fs::read_to_string(dir.path.join("child-finish")).unwrap(),
        "ControlLost"
    );
    cleanup(m, c, dir);
}
#[test]
fn privacy_does_not_hide_actual_discard_or_close_failure() {
    for mode in ["discard", "bad_cleanup"] {
        let (dir, mut m, mut c) = fixture(mode);
        wait(&mut m, &mut c, OptionalMotor::is_ready);
        c.set_camera_permission(time(), Permission::Denied).unwrap();
        let denied = c.snapshot(time()).unwrap();
        m.publish(denied);
        wait(&mut m, &mut c, OptionalMotor::reaped);
        assert!(
            !m.child
                .as_ref()
                .unwrap()
                .privacy_release_confirmed(denied.revision())
        );
        assert!(m.finish().is_err());
        cleanup(m, c, dir);
    }
}
#[test]
fn crash_and_stalled_control_never_allow_replacement_or_false_cleanup() {
    for mode in ["crash", "hang"] {
        let (dir, mut m, mut c) = fixture(mode);
        wait(&mut m, &mut c, OptionalMotor::reaped);
        assert!(!m.is_ready());
        assert!(m.finish().is_err());
        let worker = m.identity().unwrap().worker;
        m.start_before_capture(c.snapshot(time()).unwrap(), false, true);
        assert_eq!(worker, m.identity().unwrap().worker);
        cleanup(m, c, dir);
    }
}
#[test]
fn stale_snapshot_does_not_revoke_newer_idle_authority() {
    let (dir, mut m, mut c) = fixture("normal");
    wait(&mut m, &mut c, OptionalMotor::is_ready);
    let old = c.snapshot(time()).unwrap();
    let current = c.snapshot(time()).unwrap();
    m.publish(current);
    m.publish(old);
    assert!(m.is_ready());
    assert_eq!(m.guard.state().unwrap(), current);
    cleanup(m, c, dir);
}
#[test]
fn motor_crash_preserves_actual_voice_owner_and_next_audio_dispatch() {
    use super::super::{Reply, Rig, dispatch_speaker_chunk, now, service_motor};
    use crate::{
        options::{NoiseSuppression, PlayoutStart, TurnTaking},
        process::Worker,
        session_state::SessionState,
        turn_taking::PlaybackMask,
        wire::{Control as LiveControl, SpeakerChunk},
    };
    // Config freezing and all unrelated child exec barriers precede the live
    // input lease. Those setup costs must not revoke the voice turn under test.
    let (dir, prepared) = prepare_fixture("crash");
    let (speaker, mut speaker_peer) = Worker::sleeping_fixture(&dir.path, "speaker");
    let (capture, mut capture_peer) = Worker::sleeping_fixture(&dir.path, "capture");
    let (provider, mut provider_peer) = Worker::sleeping_fixture(&dir.path, "provider");
    let (privacy, mut privacy_peer) = Worker::sleeping_fixture(&dir.path, "privacy");
    let (mut controller, current, _) = super::super::tests::active_reply();
    let original = current.owner;
    let playback = current.playback;
    controller
        .set_camera_permission(time(), Permission::Allowed)
        .unwrap();
    let mut motor = OptionalMotor::new(controller.snapshot(time()).unwrap().boot(), Some(prepared));
    motor.start_before_capture(controller.snapshot(time()).unwrap(), false, true);
    let mut rig = Rig {
        microphone: crate::options::Microphone::Mic2,
        end_silence_blocks: None,
        early_end_blocks: None,
        faint_level: None,
        speaker,
        capture,
        provider,
        privacy,
        ring: None.into(),
        camera: None,
        motor: Some(motor),
        ring_channel_ceiling: None,
        diagnostic_directory: None,
        noise_suppression: NoiseSuppression::On,
        turn_taking: TurnTaking::VadOnly,
        playout_start: PlayoutStart::default(),
        endpoint: None,
    };
    let mut trace = SessionState::default();
    let until = Instant::now() + Duration::from_secs(2);
    let mut last_refresh = 0;
    while !rig.motor.as_ref().unwrap().reaped() {
        assert!(Instant::now() < until);
        if monotonic_us() - last_refresh >= 10_000 {
            // The real voice loop continues capture/admission while an optional
            // motor is starting or failing. Renew the same 500 ms input lease;
            // never replace the original owner or enlarge its permitted bound.
            let at = time();
            let input_until = at.checked_add(500_000).unwrap();
            controller
                .set_capture(at, CaptureState::RetainingUntil(input_until))
                .unwrap();
            controller
                .set_admission(at, AdmissionState::OpenUntil(input_until))
                .unwrap();
            rig.motor
                .as_mut()
                .unwrap()
                .publish(controller.snapshot(time()).unwrap());
            assert_eq!(controller.snapshot(time()).unwrap().owner(), Some(original));
            last_refresh = monotonic_us();
        }
        service_motor(&mut rig, &mut trace).unwrap();
        std::thread::park_timeout(Duration::from_millis(1));
    }
    service_motor(&mut rig, &mut trace).unwrap();
    assert!(!rig.motor.as_ref().unwrap().is_ready());
    assert!(rig.motor.as_ref().unwrap().finish().is_err());
    let mut state = controller.snapshot(now()).unwrap();
    rig.publish(state).unwrap();
    rig.running().unwrap();
    for peer in [
        &mut speaker_peer,
        &mut capture_peer,
        &mut provider_peer,
        &mut privacy_peer,
    ] {
        assert!(
            matches!(peer.control.receive::<LiveControl>().unwrap(),Some(LiveControl::Authority{snapshot}) if snapshot.owner()==Some(original))
        );
    }
    let mut reply: Option<Reply> = Some(current);
    let mut pending = None;
    let mut sequence = 0;
    dispatch_speaker_chunk(
        &mut controller,
        &mut reply,
        &mut rig.speaker.channels.data,
        &mut pending,
        &mut sequence,
        &mut state,
        &mut PlaybackMask::new(TurnTaking::VadOnly),
    )
    .unwrap();
    let chunk = speaker_peer
        .data
        .receive::<SpeakerChunk>()
        .unwrap()
        .unwrap();
    assert_eq!(chunk.permit.owner(), original);
    assert_eq!(chunk.samples, vec![123; 240]);
    assert_eq!(reply.unwrap().playback, playback);
    assert_eq!(
        trace
            .iter()
            .filter(|event| event["kind"] == "motor_degraded")
            .count(),
        1
    );
    assert!(
        !trace
            .iter()
            .any(|event| event["kind"] == "listening_ready" || event["kind"] == "reply_cancelled")
    );
    drop(rig);
    drop((speaker_peer, capture_peer, provider_peer, privacy_peer));
    fs::remove_dir_all(&dir.path).unwrap();
}
#[test]
fn unresolved_cleanup_removes_the_service_mute_restart_token() {
    use crate::service_privacy::{self, PhysicalMute};
    let (dir, mut m, mut c) = fixture("hang");
    wait(&mut m, &mut c, OptionalMotor::is_ready);
    let outcome: super::super::Result<()> = Err(Box::new(PhysicalMute {
        observed_at_us: monotonic_us(),
    }));
    m.begin_shutdown();
    assert!(!m.reaped());
    let result = service_privacy::finish(outcome, m.finish()).unwrap_err();
    assert!(result.downcast_ref::<PhysicalMute>().is_none());
    wait(&mut m, &mut c, OptionalMotor::reaped);
    assert!(m.finish().is_err());
    cleanup(m, c, dir);
}
#[test]
fn frozen_config_and_executable_are_exact_and_source_changes_are_not_adopted() {
    let (dir, mut m, mut c) = fixture("normal");
    wait(&mut m, &mut c, OptionalMotor::is_ready);
    let p = m.prepared.as_ref().unwrap();
    let bytes = fs::read(&p.config_file.path).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        p.identity.config_sha256
    );
    let executable = fs::read(&p.executable.path).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&executable)),
        p.identity.executable_sha256
    );
    fs::write(dir.path.join("input.json"), b"{}").unwrap();
    assert_eq!(fs::read(&p.config_file.path).unwrap(), bytes);
    cleanup(m, c, dir);
}
#[test]
fn motor_flags_are_paired_explicit_and_not_accepted_by_fixture_mode() {
    use crate::{fixture_provider::FixtureOptions, options::SessionOptions};
    let strings = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert!(SessionOptions::parse(&[]).unwrap().motor.is_none());
    let valid = strings(&[
        "--motor-config",
        "/private/unit.json",
        "--motor-worker",
        "/private/worker",
    ]);
    assert!(SessionOptions::parse(&valid).unwrap().motor.is_some());
    for args in [
        vec!["--motor-config", "/unit"],
        vec!["--motor-worker", "worker", "--motor-config", "/unit"],
        vec![
            "--motor-worker",
            "/worker",
            "--motor-worker",
            "/again",
            "--motor-config",
            "/unit",
        ],
    ] {
        assert!(SessionOptions::parse(&strings(&args)).is_err());
    }
    let mut fixture = valid;
    fixture.push("--diagnostics".into());
    assert!(FixtureOptions::parse(&fixture).is_err());
}
#[test]
fn pending_segment_privacy_revoke_wins_before_any_position_write() {
    let (dir, mut m, mut c) = fixture("gate_segment");
    wait(&mut m, &mut c, OptionalMotor::is_ready);
    let e = evidence(&mut m, &mut c, 1);
    m.offer_inspection(e).unwrap();
    wait(&mut m, &mut c, |m| m.inspected_at_us.is_some());
    let e = evidence(&mut m, &mut c, 2);
    m.offer_segment(MovementOffer {
        selection: JointSelection::ALL,
        evidence: e,
        target: [0.0; 5],
        duration_us: 40_000,
    })
    .unwrap();
    assert!(m.pending.is_some());
    c.set_camera_permission(time(), Permission::Denied).unwrap();
    m.publish(c.snapshot(time()).unwrap());
    assert!(m.pending.is_none());
    fs::write(dir.path.join("motor/go"), b"release inert worker").unwrap();
    wait(&mut m, &mut c, OptionalMotor::reaped);
    assert!(
        m.finish().is_ok(),
        "{:?}; supervisor={:?}; child_finish={:?}",
        m.finish(),
        m.child.as_ref().and_then(Supervisor::failure),
        fs::read_to_string(dir.path.join("child-finish"))
    );
    assert_eq!(
        fs::read_to_string(dir.path.join("child-position-writes")).unwrap(),
        "0"
    );
    cleanup(m, c, dir);
}
#[test]
fn late_started_callback_cannot_restore_an_offer_after_owner_replacement() {
    let (dir, mut m, mut c) = fixture("gate_segment");
    wait(&mut m, &mut c, OptionalMotor::is_ready);
    let e = evidence(&mut m, &mut c, 1);
    m.offer_inspection(e).unwrap();
    wait(&mut m, &mut c, |m| m.inspected_at_us.is_some());
    let e = evidence(&mut m, &mut c, 2);
    m.offer_segment(MovementOffer {
        selection: JointSelection::ALL,
        evidence: e,
        target: [0.0; 5],
        duration_us: 40_000,
    })
    .unwrap();
    let successor = c.admit(time(), AdmittedInput::NewTurn).unwrap();
    m.publish(c.snapshot(time()).unwrap());
    let late = Event::Started {
        request: e.request,
        motion: Motion {
            worker: m.identity().unwrap().worker,
            owner: e.permit.owner(),
            sequence: 1,
        },
        at_us: monotonic_us(),
    };
    m.observe(&late).unwrap();
    assert!(m.active.is_none());
    assert!(m.pending.is_none());
    assert!(!m.is_ready());
    assert_eq!(c.snapshot(time()).unwrap().owner(), Some(successor));
    fs::write(dir.path.join("motor/go"), b"release inert worker").unwrap();
    wait(&mut m, &mut c, OptionalMotor::reaped);
    assert_eq!(
        fs::read_to_string(dir.path.join("child-position-writes")).unwrap(),
        "0"
    );
    cleanup(m, c, dir);
}

#[test]
fn explicit_current_segment_reaches_inert_counts_without_completing_voice() {
    let (dir, mut motor, mut controller) = fixture("normal");
    wait(&mut motor, &mut controller, OptionalMotor::is_ready);
    let inspected = evidence(&mut motor, &mut controller, 1);
    motor.offer_inspection(inspected).unwrap();
    wait(&mut motor, &mut controller, |m| m.inspected_at_us.is_some());
    let offered = evidence(&mut motor, &mut controller, 2);
    let original_owner = offered.permit.owner();
    // Synthetic normalized values only; the emitted mask is real protocol
    // evidence from an inert child, not physical placement qualification.
    let started = Instant::now();
    motor
        .offer_segment(MovementOffer {
            selection: JointSelection::from_mask(0x10).unwrap(),
            evidence: offered,
            target: [0.0, 0.0, 0.0, 2.0, -2.0],
            duration_us: 80_000,
        })
        .unwrap();
    eprintln!(
        "motor_validated_offer_to_channel_us={}",
        started.elapsed().as_micros()
    );
    wait(&mut motor, &mut controller, |m| {
        m.pending.is_none() && m.active.is_none()
    });
    assert!(motor.is_ready());
    assert!(
        motor.inspected_at_us.is_none(),
        "next gesture requires a new inspection"
    );
    assert_eq!(
        controller.snapshot(time()).unwrap().owner(),
        Some(original_owner)
    );
    assert!(controller.snapshot(time()).unwrap().input_active());
    motor.begin_shutdown();
    wait(&mut motor, &mut controller, OptionalMotor::reaped);
    assert!(motor.finish().is_ok());
    let writes: usize = fs::read_to_string(dir.path.join("child-position-writes"))
        .unwrap()
        .parse()
        .unwrap();
    assert!(writes > 0 && writes <= 20);
    let counts: [u16; 5] =
        serde_json::from_slice(&fs::read(dir.path.join("child-final-counts")).unwrap()).unwrap();
    assert_eq!(counts, [2000, 2000, 2000, 2000, 1980]);
    assert_eq!(
        fs::read_to_string(dir.path.join("child-emitted-mask")).unwrap(),
        "16"
    );
    cleanup(motor, controller, dir);
}

#[test]
fn same_owner_input_end_cancels_queued_gesture_before_inert_bus_write() {
    let (dir, mut motor, mut controller) = fixture("gate_segment");
    wait(&mut motor, &mut controller, OptionalMotor::is_ready);
    let inspected = evidence(&mut motor, &mut controller, 1);
    motor.offer_inspection(inspected).unwrap();
    wait(&mut motor, &mut controller, |m| m.inspected_at_us.is_some());
    let offered = evidence(&mut motor, &mut controller, 2);
    motor
        .offer_segment(MovementOffer {
            selection: JointSelection::ALL,
            evidence: offered,
            target: [0.0, 0.0, 0.0, 2.0, -2.0],
            duration_us: 80_000,
        })
        .unwrap();
    controller
        .input_ended(time(), offered.permit.owner())
        .unwrap();
    motor.publish(controller.snapshot(time()).unwrap());
    assert!(!motor.is_ready());
    assert!(motor.pending.is_none());
    assert_eq!(
        controller.snapshot(time()).unwrap().owner(),
        Some(offered.permit.owner())
    );
    fs::write(dir.path.join("motor/go"), b"release inert child").unwrap();
    wait(&mut motor, &mut controller, OptionalMotor::reaped);
    assert_eq!(
        fs::read_to_string(dir.path.join("child-position-writes")).unwrap(),
        "0"
    );
    cleanup(motor, controller, dir);
}

#[test]
fn failed_reap_after_stopped_is_latched_and_reported_once() {
    use super::super::{Rig, service_motor};
    use crate::{
        options::{NoiseSuppression, PlayoutStart, TurnTaking},
        process::Worker,
        session_state::SessionState,
    };
    let (dir, mut motor, mut controller) = fixture("gated_exit_failure");
    wait(&mut motor, &mut controller, OptionalMotor::is_ready);
    motor.begin_shutdown();
    let deadline = Instant::now() + Duration::from_secs(1);
    let mut stopped = false;
    while !stopped {
        assert!(
            Instant::now() < deadline,
            "gated child did not report Stopped"
        );
        motor.poll(|event| stopped |= matches!(event, Event::Stopped { .. }));
        std::thread::park_timeout(Duration::from_millis(1));
    }
    assert!(motor.failure().is_none());
    assert!(
        !motor.reaped(),
        "child is held after its valid stop receipt"
    );
    fs::write(
        dir.path.join("motor/go"),
        b"explicit nonzero exit after Stopped",
    )
    .unwrap();
    // Drive the real supervisor to its observed exit boundary, independently of
    // the optional adapter. Its valid Stopped receipt was already consumed, so
    // poll skips the closed socket on every platform. A nonzero exit discovered
    // afterward is a real stored-status fault without a returned Err or Event.
    let deadline = Instant::now() + Duration::from_secs(1);
    let child = motor.child.as_mut().unwrap();
    while !child.reaped() {
        assert!(Instant::now() < deadline, "gated child not reaped");
        assert!(
            child.poll().unwrap().is_none(),
            "crash must have no new event"
        );
        std::thread::park_timeout(Duration::from_millis(1));
    }
    assert_eq!(child.failure(), Some("motor child failed or was killed"));
    assert!(
        child.poll().unwrap().is_none(),
        "quiescent poll still returns no event"
    );
    assert!(
        motor.failure().is_none(),
        "adapter has not yet observed stored failure"
    );
    let (speaker, _) = Worker::sleeping_fixture(&dir.path, "speaker");
    let (capture, _) = Worker::sleeping_fixture(&dir.path, "capture");
    let (provider, _) = Worker::sleeping_fixture(&dir.path, "provider");
    let (privacy, _) = Worker::sleeping_fixture(&dir.path, "privacy");
    let mut rig = Rig {
        microphone: crate::options::Microphone::Mic2,
        end_silence_blocks: None,
        early_end_blocks: None,
        faint_level: None,
        speaker,
        capture,
        provider,
        privacy,
        ring: None.into(),
        camera: None,
        motor: Some(motor),
        ring_channel_ceiling: None,
        diagnostic_directory: None,
        noise_suppression: NoiseSuppression::On,
        turn_taking: TurnTaking::VadOnly,
        playout_start: PlayoutStart::default(),
        endpoint: None,
    };
    let mut trace = SessionState::default();
    service_motor(&mut rig, &mut trace).unwrap();
    assert_eq!(
        rig.motor.as_ref().unwrap().failure(),
        Some("motor child failed or was killed")
    );
    assert!(!rig.motor.as_ref().unwrap().is_ready());
    assert!(rig.motor.as_ref().unwrap().finish().is_err());
    service_motor(&mut rig, &mut trace).unwrap();
    assert_eq!(
        trace
            .iter()
            .filter(|e| e["kind"] == "motor_degraded")
            .count(),
        1
    );
    drop(rig);
    fs::remove_dir_all(&dir.path).unwrap();
}

mod native_head;
