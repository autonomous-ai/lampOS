//! Actual child/channel startup with fake peers: no device or provider access.
use super::*;
use std::os::unix::net::UnixDatagram;

pub(crate) struct ChildFixture {
    pub directory: SessionDirectory,
    pub executable: PathBuf,
    identity: PathBuf,
}
impl ChildFixture {
    pub fn new() -> Self {
        let directory = SessionDirectory::create().unwrap();
        let executable = directory.path.join("fake-ring");
        let identity = directory.path.join("identity");
        fs::write(&executable, format!(
            "#!/bin/sh\nif [ \"$1\" = --fixture-preflight ]; then exit 0; fi\nprintf '%s\\n%s\\n%s\\n' \"$4\" \"$5\" \"$$\" > '{}'\nexec /bin/sleep 60\n",
            identity.display()
        )).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        // macOS checks a fresh executable on its first exec, which took over
        // the 3 s worker startup bound on a loaded Mac (2026-10-15). Pay that
        // here, before any startup deadline runs; this arm writes nothing.
        // On Linux a sibling's fork may still hold the new file open for
        // writing (`ETXTBSY`), which `status_of_fresh` waits out.
        let preflight = super::status_of_fresh(
            Command::new(&executable)
                .arg("--fixture-preflight")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null()),
        );
        assert!(preflight.success(), "fake ring preflight: {preflight}");
        Self {
            directory,
            executable,
            identity,
        }
    }
    pub fn identity(&self) -> Option<(BootId, BootId, u32)> {
        let text = match fs::read_to_string(&self.identity) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
            Err(error) => panic!("fixture identity failed: {error}"),
        };
        let lines: Vec<_> = text.lines().collect();
        if lines.len() != 3 {
            return None;
        }
        Some((
            parse_boot(lines[0]).unwrap(),
            parse_boot(lines[1]).unwrap(),
            lines[2].parse().unwrap(),
        ))
    }
    pub fn connect_peer(&self) -> Option<WorkerChannels> {
        let (parent, boot, _) = self.identity()?;
        let path = self.directory.path.join("ring");
        let mut peer = WorkerChannels::bind(&path, "ring", parent, boot, true).unwrap();
        peer.connect(&path, "ring", true).unwrap();
        Some(peer)
    }
    pub fn assert_reaped(&self) {
        let (_, _, pid) = self.identity().expect("child executed");
        assert_eq!(
            rustix::process::waitpid(
                rustix::process::Pid::from_raw(pid as i32),
                rustix::process::WaitOptions::NOHANG,
            )
            .unwrap_err(),
            rustix::io::Errno::CHILD,
            "owned child must already be reaped"
        );
    }
}
impl Drop for ChildFixture {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.executable);
        let _ = fs::remove_file(&self.identity);
        let _ = fs::remove_file(self.identity.with_extension("tmp"));
    }
}

fn unavailable(startup: RingStartup) -> (Option<Box<Worker>>, io::Error) {
    match startup {
        RingStartup::Unavailable { worker, error } => {
            eprintln!("ring unavailable fixture: {error}");
            (worker, error)
        }
        RingStartup::Ready(_) => panic!("unexpected ready ring"),
    }
}

/// A tick of a test of the startup deadline: once what the test is about has
/// happened (the child started, its peers made), the wait's clock moves on
/// 100 ms, so the 3 s deadline passes in about 30 ticks instead of 3 s. Not
/// before: a child slow to start must not find the deadline already gone.
fn move_on_once(ready: bool) {
    if ready {
        super::startup_clock::move_on(Duration::from_millis(100));
    }
}

pub(crate) fn await_exit(worker: &mut Worker) -> ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        if let Some(status) = worker.try_exit().unwrap() {
            return status;
        }
        assert!(Instant::now() < deadline, "fake child did not exit");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn failed_handshake_retains_owned_child_and_cleans_only_its_socket_namespace() {
    let fixture = ChildFixture::new();
    let mut peer = None;
    let started = Worker::spawn_optional_ring_with_tick(
        &fixture.executable,
        &fixture.directory.path,
        new_boot().unwrap(),
        &[],
        || {
            if peer.is_none() {
                peer = fixture.connect_peer();
                if let Some(peer) = &mut peer {
                    peer.control.send(WorkerEvent::Fault {
                        code: "fake SPI open failure".into(),
                    })?;
                }
            }
            Ok(())
        },
    )
    .unwrap();
    let (worker, error) = unavailable(started);
    assert!(error.to_string().contains("unexpected worker startup"));
    let mut worker = worker.expect("unready child must remain owned");
    worker.check_running().unwrap();
    worker.request_termination().unwrap();
    let status = await_exit(&mut worker);
    assert!(worker.exit_result(status).is_err());
    fixture.assert_reaped();
    assert!(!fixture.directory.path.join("ring/ring.c.w").exists());
    assert!(!fixture.directory.path.join("ring/ring.d.w").exists());
    assert!(
        fixture.directory.path.join("ring/ring.c.p").exists(),
        "owned parent endpoint remains until its Worker drops"
    );
    drop(worker);
    assert!(!fixture.directory.path.join("ring").exists());
    // Stale test peer Drop cannot remove anything in a later namespace.
    drop(peer);
}

#[test]
fn ready_ring_keeps_private_handshake_and_control_contract() {
    let fixture = ChildFixture::new();
    let mut peer = None;
    let started = Worker::spawn_optional_ring_with_tick(
        &fixture.executable,
        &fixture.directory.path,
        new_boot().unwrap(),
        &[],
        || {
            if peer.is_none() {
                peer = fixture.connect_peer();
                if let Some(peer) = &mut peer {
                    peer.control.send(WorkerEvent::Ready)?;
                }
            }
            Ok(())
        },
    )
    .unwrap();
    let RingStartup::Ready(mut worker) = started else {
        panic!("ready handshake rejected");
    };
    worker.channels.control.send(Control::Stop).unwrap();
    assert!(matches!(
        peer.as_mut().unwrap().control.receive().unwrap(),
        Some(Control::Stop)
    ));
    worker.request_termination().unwrap();
    let status = await_exit(&mut worker);
    assert!(worker.exit_result(status).is_err());
    drop(worker);
    fixture.assert_reaped();
    assert!(!fixture.directory.path.join("ring").exists());
}

/// What the lamp runs: the startup wait reads the host's clock. In a test
/// build that holds until the test's own thread moves it, and only that
/// thread's wait sees the move, so every other test starts its workers on
/// the host's clock as before (the first builder, #1412).
#[test]
fn the_startup_wait_reads_the_hosts_clock_unless_this_thread_moves_it() {
    let before = Instant::now();
    let read = startup_now();
    assert!(before <= read && read <= Instant::now());
    std::thread::spawn(|| super::startup_clock::move_on(Duration::from_secs(60)))
        .join()
        .unwrap();
    assert!(startup_now() <= Instant::now(), "another thread's move");
    super::startup_clock::move_on(Duration::from_secs(60));
    let before = Instant::now();
    let read = startup_now();
    assert!(read >= before + Duration::from_secs(60));
    assert!(read <= Instant::now() + Duration::from_secs(60));
}

#[test]
fn missing_socket_deadline_is_unchanged_and_does_not_lose_the_child() {
    let fixture = ChildFixture::new();
    let began = startup_now();
    let mut ticks = 0;
    let (worker, error) = unavailable(
        Worker::spawn_optional_ring_with_tick(
            &fixture.executable,
            &fixture.directory.path,
            new_boot().unwrap(),
            &[],
            || {
                ticks += 1;
                move_on_once(fixture.identity().is_some());
                Ok(())
            },
        )
        .unwrap(),
    );
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(startup_now() - began >= WORKER_STARTUP_TIMEOUT);
    assert!(ticks > 1);
    let mut worker = worker.unwrap();
    worker.check_running().unwrap();
    worker.request_termination().unwrap();
    let status = await_exit(&mut worker);
    assert!(worker.exit_result(status).is_err());
    drop(worker);
    fixture.assert_reaped();
    assert!(!fixture.directory.path.join("ring").exists());
}

#[test]
fn mandatory_interrupt_during_ring_startup_preserves_kind_and_reaps() {
    let fixture = ChildFixture::new();
    let error = Worker::spawn_optional_ring_with_tick(
        &fixture.executable,
        &fixture.directory.path,
        new_boot().unwrap(),
        &[],
        || {
            if fixture.identity().is_some() {
                Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "required termination",
                ))
            } else {
                Ok(())
            }
        },
    )
    .err()
    .expect("mandatory failure must remain terminal");
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    assert_eq!(error.to_string(), "required termination");
    fixture.assert_reaped();
    assert!(!fixture.directory.path.join("ring").exists());
}

#[test]
fn instant_optional_exec_failure_cannot_mask_new_mandatory_fault() {
    let directory = SessionDirectory::create().unwrap();
    let mut ticks = 0;
    let error = Worker::spawn_optional_ring_with_tick(
        &directory.path.join("absent"),
        &directory.path,
        new_boot().unwrap(),
        &[],
        || {
            ticks += 1;
            if ticks == 2 {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "physical privacy fault",
                ))
            } else {
                Ok(())
            }
        },
    )
    .err()
    .expect("mandatory error after failed exec must be terminal");
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(error.to_string(), "physical privacy fault");
    assert!(!directory.path.join("ring").exists());
}

#[test]
fn cleanup_refuses_replaced_peer_inode_and_retains_failure() {
    let fixture = ChildFixture::new();
    let mut peer = None;
    let (worker, _) = unavailable(
        Worker::spawn_optional_ring_with_tick(
            &fixture.executable,
            &fixture.directory.path,
            new_boot().unwrap(),
            &[],
            || {
                if peer.is_none() {
                    peer = fixture.connect_peer();
                    if let Some(peer) = &mut peer {
                        peer.control.send(WorkerEvent::Fault {
                            code: "fake startup failure".into(),
                        })?;
                    }
                }
                Ok(())
            },
        )
        .unwrap(),
    );
    let mut worker = worker.unwrap();
    worker.request_termination().unwrap();
    let status = await_exit(&mut worker);
    let replacement = fixture.directory.path.join("ring/ring.c.w");
    fs::remove_file(&replacement).unwrap();
    let replacement_socket = UnixDatagram::bind(&replacement).unwrap();
    fs::set_permissions(&replacement, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(
        worker
            .exit_result(status)
            .unwrap_err()
            .to_string()
            .contains("inode changed")
    );
    drop(worker);
    assert!(replacement.exists(), "never erase a replacement socket");
    drop(peer);
    assert!(
        replacement.exists(),
        "old Endpoint drop must preserve replacement"
    );
    drop(replacement_socket);
    fs::remove_file(replacement).unwrap();
    fs::remove_dir(fixture.directory.path.join("ring")).unwrap();
    fixture.assert_reaped();
}

#[test]
fn cleanup_refuses_unexpected_file_without_erasing_it() {
    let fixture = ChildFixture::new();
    let file = fixture.directory.path.join("ring/ring.c.w");
    let (worker, error) = unavailable(
        Worker::spawn_optional_ring_with_tick(
            &fixture.executable,
            &fixture.directory.path,
            new_boot().unwrap(),
            &[],
            || {
                if fixture.identity().is_some() && !file.exists() {
                    fs::write(&file, b"not a worker socket")?;
                }
                Ok(())
            },
        )
        .unwrap(),
    );
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    let mut worker = worker.unwrap();
    worker.request_termination().unwrap();
    let status = await_exit(&mut worker);
    assert!(worker.exit_result(status).is_err());
    drop(worker);
    assert!(file.exists());
    fs::remove_file(file).unwrap();
    fs::remove_dir(fixture.directory.path.join("ring")).unwrap();
    fixture.assert_reaped();
}

#[test]
fn permanently_nonprivate_peers_timeout_without_connecting_or_chmod() {
    let fixture = ChildFixture::new();
    let mut peers = None;
    let (worker, error) = unavailable(
        Worker::spawn_optional_ring_with_tick(
            &fixture.executable,
            &fixture.directory.path,
            new_boot().unwrap(),
            &[],
            || {
                if fixture.identity().is_some() && peers.is_none() {
                    let c = fixture.directory.path.join("ring/ring.c.w");
                    let d = fixture.directory.path.join("ring/ring.d.w");
                    peers = Some((UnixDatagram::bind(&c)?, UnixDatagram::bind(&d)?));
                    for path in [&c, &d] {
                        fs::set_permissions(path, fs::Permissions::from_mode(0o666))?;
                    }
                }
                move_on_once(peers.is_some());
                Ok(())
            },
        )
        .unwrap(),
    );
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    let mut worker = worker.unwrap();
    assert_eq!(
        fs::metadata(fixture.directory.path.join("ring/ring.c.w"))
            .unwrap()
            .mode()
            & 0o777,
        0o666
    );
    assert_eq!(
        worker
            .channels
            .control
            .send(Control::Stop)
            .unwrap_err()
            .kind(),
        io::ErrorKind::NotConnected
    );
    worker.request_termination().unwrap();
    let status = await_exit(&mut worker);
    assert!(worker.exit_result(status).is_err());
    drop(worker);
    drop(peers);
    fixture.assert_reaped();
    assert!(!fixture.directory.path.join("ring").exists());
}

#[test]
fn connected_but_unready_ring_times_out_and_remains_owned() {
    let fixture = ChildFixture::new();
    let mut peer = None;
    let (worker, error) = unavailable(
        Worker::spawn_optional_ring_with_tick(
            &fixture.executable,
            &fixture.directory.path,
            new_boot().unwrap(),
            &[],
            || {
                if peer.is_none() {
                    peer = fixture.connect_peer();
                }
                move_on_once(peer.is_some());
                Ok(())
            },
        )
        .unwrap(),
    );
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(error.to_string().contains("readiness timed out"));
    let mut worker = worker.unwrap();
    worker.check_running().unwrap();
    worker.request_termination().unwrap();
    let status = await_exit(&mut worker);
    assert!(worker.exit_result(status).is_err());
    drop(worker);
    fixture.assert_reaped();
    assert!(!fixture.directory.path.join("ring").exists());
}

#[test]
fn child_exit_before_ready_is_optional_but_abnormal_exit_is_retained() {
    let fixture = ChildFixture::new();
    let script = fs::read_to_string(&fixture.executable)
        .unwrap()
        .replace("exec /bin/sleep 60", "exit 23");
    fs::write(&fixture.executable, script).unwrap();
    let (worker, error) = unavailable(
        Worker::spawn_optional_ring_with_tick(
            &fixture.executable,
            &fixture.directory.path,
            new_boot().unwrap(),
            &[],
            || Ok(()),
        )
        .unwrap(),
    );
    assert!(error.to_string().contains("worker exited"));
    let mut worker = worker.unwrap();
    let status = worker.try_exit().unwrap().unwrap();
    assert_eq!(status.code(), Some(23));
    assert!(
        worker
            .exit_result(status)
            .unwrap_err()
            .to_string()
            .contains("exited abnormally")
    );
    drop(worker);
    fixture.assert_reaped();
    assert!(!fixture.directory.path.join("ring").exists());
}
