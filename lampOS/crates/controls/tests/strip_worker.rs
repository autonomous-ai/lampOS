use lamp_controls::strip_worker::{
    DeviceClaim, MAX_PACKET_BYTES, Packet, SILENCE_US, STARTUP_US, StripReader, Worker,
};
use lamp_msgs::{ClockStamp, ControlFault};
use std::{
    fs, io,
    os::unix::net::UnixDatagram,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "ctw-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// The fixture is this Rust test executable; no shell, sleep program, I2C or
// external interpreter is needed. Extra args after -- are harmless libtest
// filters. The explicit exact fixture name ensures that this test runs.
#[test]
fn worker_fixture() {
    let Ok(mode) = std::env::var("LAMP_CONTROLS_WORKER_FIXTURE") else {
        return;
    };
    let args: Vec<_> = std::env::args().collect();
    let value = |name: &str| args.windows(2).find(|pair| pair[0] == name).unwrap()[1].clone();
    if mode == "silent" {
        thread::sleep(Duration::from_secs(10));
        return;
    }
    let _claim = if mode == "claim" {
        Some(
            DeviceClaim::at(std::path::Path::new(
                &std::env::var("LAMP_CONTROLS_CLAIM_FIXTURE").unwrap(),
            ))
            .unwrap(),
        )
    } else {
        None
    };
    let socket = UnixDatagram::unbound().unwrap();
    socket.connect(value("--sink")).unwrap();
    socket.set_nonblocking(true).unwrap();
    let mut clock = value("--clock-id");
    let mut session = value("--session");
    if mode == "clock" {
        clock = "other-clock".into();
    }
    if mode == "session" {
        session = "other-reader".into();
    }
    let now = lamp_ipc::monotonic_us();
    let at_us = match mode.as_str() {
        "stale" => now - 100_000,
        "future" => now + 1_000_000,
        _ => now,
    };
    let stamp = ClockStamp {
        clock_id: clock,
        at_us,
        sequence: 1,
    };
    let packet = if mode == "fault" {
        Packet::Fault {
            stamp,
            session_id: session,
            detail: "I2C failed".into(),
        }
    } else {
        Packet::Sample {
            stamp,
            session_id: session,
            mask: if mode == "mask" { 0x8000 } else { 7 },
        }
    };
    let bytes = match mode.as_str() {
        "oversized" => vec![b'x'; MAX_PACKET_BYTES + 1],
        "corrupt" => b"broken".to_vec(),
        _ => serde_json::to_vec(&packet).unwrap(),
    };
    socket.send(&bytes).unwrap();
    thread::sleep(Duration::from_secs(10));
}

fn spawn(root: &Root, mode: &str) -> Worker {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .arg("--exact")
        .arg("worker_fixture")
        .arg("--nocapture")
        .arg("--")
        .env("LAMP_CONTROLS_WORKER_FIXTURE", mode);
    Worker::spawn(
        command,
        &root.0,
        "worker-clock",
        "worker-session",
        lamp_ipc::monotonic_us(),
    )
    .unwrap()
}

fn receive(
    worker: &mut Worker,
) -> Result<lamp_controls::touch::StripSample, lamp_controls::input::InputError> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(sample) = worker.read(lamp_ipc::monotonic_us())? {
            return Ok(sample);
        }
        assert!(
            Instant::now() < deadline,
            "Rust worker fixture sent no result"
        );
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn acquisition_keeps_worker_time_and_sequence_then_child_is_reaped() {
    let root = Root::new();
    let mut worker = spawn(&root, "sample");
    let sample = receive(&mut worker).unwrap();
    assert_eq!(sample.mask, 7);
    assert_eq!(sample.stamp.sequence, 1);
    assert_eq!(sample.session_id, "worker-session");
    assert!(sample.stamp.at_us <= lamp_ipc::monotonic_us());
    let cleanup = worker.shutdown();
    assert!(cleanup.kill_requested && cleanup.exit_observed);
    assert_eq!(cleanup.success, Some(false));
    assert!(cleanup.error.is_none());
    drop(worker);
    assert_eq!(fs::read_dir(&root.0).unwrap().count(), 0);
}

#[test]
fn malformed_faulted_or_mismatched_workers_never_become_releases() {
    for (mode, fault) in [
        ("stale", ControlFault::Stale),
        ("future", ControlFault::Future),
        ("clock", ControlFault::WrongClock),
        ("session", ControlFault::Invalid),
        ("mask", ControlFault::Invalid),
        ("fault", ControlFault::Missing),
        ("oversized", ControlFault::Overflow),
        ("corrupt", ControlFault::Invalid),
    ] {
        let root = Root::new();
        let mut worker = spawn(&root, mode);
        assert_eq!(receive(&mut worker).unwrap_err().fault, fault, "{mode}");
        assert!(worker.read(lamp_ipc::monotonic_us()).unwrap().is_none());
        assert!(worker.shutdown().exit_observed, "{mode}");
    }
}

#[test]
fn silent_worker_has_bounded_startup_and_never_restarts() {
    let root = Root::new();
    let mut worker = spawn(&root, "silent");
    let now = lamp_ipc::monotonic_us();
    for _ in 0..1000 {
        assert!(worker.read(now).unwrap().is_none());
    }
    assert_eq!(
        worker.read(now + STARTUP_US).unwrap_err().fault,
        ControlFault::Stale
    );
    for _ in 0..1000 {
        assert!(worker.read(now + STARTUP_US + 1).unwrap().is_none());
    }
    assert!(worker.shutdown().exit_observed);
}

#[test]
fn failed_spawn_cleans_only_its_socket_and_does_not_remove_foreign_files() {
    let root = Root::new();
    fs::write(root.0.join("preserve"), "owned by another path").unwrap();
    let failed = Worker::spawn(
        Command::new(root.0.join("missing-executable")),
        &root.0,
        "clock",
        "session",
        1,
    );
    assert!(matches!(failed, Err(ref error) if error.kind() == io::ErrorKind::NotFound));
    assert_eq!(fs::read_dir(&root.0).unwrap().count(), 1);
    assert_eq!(
        fs::read_to_string(root.0.join("preserve")).unwrap(),
        "owned by another path"
    );
}

/// #7402: a strip worker whose program is missing names it, its kind kept:
/// a bare `No such file or directory (os error 2)` did not say which.
#[test]
fn a_strip_worker_spawn_of_a_missing_program_names_it() {
    let root = Root::new();
    let missing = root.0.join("missing-executable");
    let error = Worker::spawn(Command::new(&missing), &root.0, "clock", "session", 1)
        .err()
        .expect("a missing program is not started");
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert!(
        error
            .to_string()
            .starts_with(&format!("spawn {}: ", missing.display())),
        "{error}"
    );
}

#[test]
fn an_existing_socket_path_is_never_unlinked_by_a_second_worker() {
    let root = Root::new();
    let path = root
        .0
        .join(format!("controls-mpr-{}-123.sock", std::process::id()));
    let owner = UnixDatagram::bind(&path).unwrap();
    assert!(Worker::spawn(Command::new("unused"), &root.0, "clock", "session", 123).is_err());
    assert!(path.exists());
    drop(owner);
}

#[test]
fn queued_fresh_packets_cannot_rescue_expired_startup_or_silence() {
    for established in [false, true] {
        let root = Root::new();
        let now = lamp_ipc::monotonic_us();
        let mut worker = spawn(&root, "silent");
        let path = fs::read_dir(&root.0)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let sender = UnixDatagram::unbound().unwrap();
        sender.connect(path).unwrap();
        let packet = |at_us, sequence| Packet::Sample {
            stamp: ClockStamp {
                clock_id: "worker-clock".into(),
                at_us,
                sequence,
            },
            session_id: "worker-session".into(),
            mask: 0,
        };
        let first = lamp_ipc::monotonic_us();
        if established {
            lamp_controls::strip_worker::send(&sender, &packet(first, 1)).unwrap();
            assert!(worker.read(first).unwrap().is_some());
        }
        let expired = if established {
            first + SILENCE_US
        } else {
            now + STARTUP_US + 1_000_000
        };
        lamp_controls::strip_worker::send(&sender, &packet(expired, 2)).unwrap();
        assert_eq!(worker.read(expired).unwrap_err().fault, ControlFault::Stale);
        assert!(worker.read(expired + 1).unwrap().is_none());
        assert!(worker.shutdown().exit_observed);
    }
}

#[test]
fn device_claim_is_exclusive_until_the_child_really_exits() {
    let root = Root::new();
    let second_root = Root::new();
    let path = second_root.0.join("shared-device.lock");
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .arg("--exact")
        .arg("worker_fixture")
        .arg("--nocapture")
        .arg("--")
        .env("LAMP_CONTROLS_WORKER_FIXTURE", "claim")
        .env("LAMP_CONTROLS_CLAIM_FIXTURE", &path);
    let mut worker = Worker::spawn(
        command,
        &root.0,
        "worker-clock",
        "worker-session",
        lamp_ipc::monotonic_us(),
    )
    .unwrap();
    receive(&mut worker).unwrap();
    assert!(DeviceClaim::at(&path).is_err());
    assert!(worker.shutdown().exit_observed);
    let after_exit = DeviceClaim::at(&path).unwrap();
    assert!(DeviceClaim::at(&path).is_err());
    drop(after_exit);
    assert!(
        path.exists(),
        "claim inode must never be unlinked on release"
    );
    assert!(DeviceClaim::at(&path).is_ok());
}

#[test]
fn device_claim_refuses_symlinks_hardlinks_and_shared_mode() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let root = Root::new();
    let path = root.0.join("device.lock");
    drop(DeviceClaim::at(&path).unwrap());
    let alias = root.0.join("alias");
    symlink(&path, &alias).unwrap();
    assert!(DeviceClaim::at(&alias).is_err());
    fs::remove_file(&alias).unwrap();
    fs::hard_link(&path, &alias).unwrap();
    assert!(DeviceClaim::at(&path).is_err());
    fs::remove_file(&alias).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o666)).unwrap();
    assert!(DeviceClaim::at(&path).is_err());
}
