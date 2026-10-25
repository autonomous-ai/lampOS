use super::*;
use crate::{
    Joint,
    calibration::{CalibrationRecord, UnitCalibration, UnitId},
    control::{BusPort, MotorControl, Timing},
    runtime::{
        wire,
        worker::{self, Worker},
    },
};
use lamp_turn_owner::{Controller, Permission};
use std::time::{Duration, Instant};

struct Inert;
impl BusPort for Inert {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        panic!("idle child read hardware")
    }
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        panic!("idle child wrote hardware")
    }
    fn discard_output(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[test]
fn inert_child() {
    let Ok(dir) = std::env::var("LAMP_MOTOR_TEST_DIRECTORY") else {
        return;
    };
    let directory = PathBuf::from(dir);
    let parent = parse_boot(&std::env::var("LAMP_MOTOR_TEST_PARENT").unwrap()).unwrap();
    let own = parse_boot(&std::env::var("LAMP_MOTOR_TEST_WORKER").unwrap()).unwrap();
    let mode = std::env::var("LAMP_MOTOR_TEST_MODE").unwrap();
    if mode == "crash_partial" {
        let _control = lamp_ipc::Endpoint::bind(directory.join("motor.c.w")).unwrap();
        std::process::exit(7);
    }
    let unit = UnitId::new("inert-child").unwrap();
    let calibration = UnitCalibration::new(
        unit.clone(),
        Joint::ALL.map(|joint| CalibrationRecord {
            joint,
            id: joint.id() as u16,
            drive_mode: 0,
            homing_offset: 0,
            range_min: 1000,
            range_max: 3000,
        }),
    )
    .unwrap();
    let core = MotorControl::new(
        Inert,
        SystemClock,
        parent,
        own,
        &unit,
        calibration,
        Timing::new(40_000, 5_000, 200_000, 500_000, 10_000).unwrap(),
    )
    .unwrap();
    // The core is built before the endpoints are bound: the parent issues
    // Authority as soon as it can connect, and a datagram that waits in the
    // queue past MAX_AGE_US while this child starts is refused as stale.
    let mut channels = Channels::bind(&directory, parent, own, true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !channels.connect_step().unwrap() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1))
    }
    if mode == "crash" {
        std::process::exit(7)
    }
    if mode == "hang" {
        loop {
            std::thread::park_timeout(Duration::from_secs(1));
        }
    }
    // The worker only once connected: Worker::new starts its
    // INITIAL_AUTHORITY_US clock, which must not run during the connect.
    let worker = Worker::new(core, SystemClock, parent, own, own, 1_000_000).unwrap();
    // Then a Progress, the worker's own heartbeat, says it reads from here
    // (`ready`): an Authority sent at the parent's connect could wait past
    // MAX_AGE_US behind a stall in this child.
    let n = SystemClock.now();
    channels
        .control
        .send(
            &Event::Progress {
                at_us: n.as_micros(),
            },
            n,
        )
        .unwrap();
    let result = if mode == "hang_after_ready" {
        struct HangAfterReady(Channels);
        impl worker::Link for HangAfterReady {
            fn control(
                &mut self,
                now: &mut dyn FnMut() -> io::Result<lamp_turn_owner::MonoTime>,
            ) -> io::Result<Option<Control>> {
                self.0.control.receive_with(now)
            }
            fn command(
                &mut self,
                now: &mut dyn FnMut() -> io::Result<lamp_turn_owner::MonoTime>,
            ) -> io::Result<Option<MotorCommand>> {
                self.0.data.receive_with(now)
            }
            fn publish(&mut self, event: &Event, n: lamp_turn_owner::MonoTime) -> io::Result<()> {
                self.0.control.send(event, n)?;
                if matches!(event, Event::Ready { .. }) {
                    loop {
                        std::thread::park_timeout(Duration::from_secs(1));
                    }
                }
                Ok(())
            }
        }
        worker::run(worker, &mut HangAfterReady(channels))
    } else {
        worker::run(worker, &mut channels)
    };
    if mode == "parent_lost" {
        assert!(result.is_err());
    } else {
        result.unwrap();
    }
}
fn fixture(mode: &str) -> (Supervisor, Controller) {
    let parent = new_boot().unwrap();
    let worker = new_boot().unwrap();
    let directory = PathBuf::from("/tmp").join(format!("mw-{}", boot_text(worker)));
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args([
        "--exact",
        "runtime::supervisor::tests::inert_child",
        "--nocapture",
    ])
    .env("LAMP_MOTOR_TEST_DIRECTORY", &directory)
    .env("LAMP_MOTOR_TEST_PARENT", boot_text(parent))
    .env("LAMP_MOTOR_TEST_WORKER", boot_text(worker))
    .env("LAMP_MOTOR_TEST_MODE", mode);
    let supervisor = Supervisor::spawn_command(cmd, &directory, parent, worker).unwrap();
    let mut controller = Controller::new(parent, SystemClock.now());
    controller
        .set_microphone_permission(SystemClock.now(), Permission::Allowed)
        .unwrap();
    controller
        .set_camera_permission(SystemClock.now(), Permission::Allowed)
        .unwrap();
    (supervisor, controller)
}
fn until(s: &mut Supervisor, mut f: impl FnMut(&mut Supervisor) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !f(s) {
        assert!(
            Instant::now() < deadline,
            "finite fixture timeout: {:?}",
            s.failure()
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
/// One Authority, as production sends one per revision (`send_authority`),
/// once the child's first Progress says it is reading (`inert_child`). One
/// per poll from the connect filled the child's socket, and one at the
/// connect went stale behind a stall in the child.
fn ready(s: &mut Supervisor, c: &mut Controller) {
    let mut sent = false;
    until(s, |s| {
        let reading = matches!(s.poll().unwrap(), Some(Event::Progress { .. }));
        if reading && !sent {
            s.send_control(&Control::Authority {
                snapshot: c.snapshot(SystemClock.now()).unwrap(),
            })
            .unwrap();
            sent = true;
        }
        s.is_ready()
    });
}

#[test]
fn qualification_rejects_wrong_supervisor_before_any_channel_dispatch() {
    use crate::control::{EncoderReadback, Readback};
    use crate::runtime::qualification::{HeadPlan, HeadQualification, OriginalEvidence, Phase};
    use lamp_turn_owner::{AdmissionState, AdmittedInput, CaptureState, OutputKind};

    // The real Supervisor owns a bounded inert child. Its command/control
    // channels are replaced with real socket pairs so each publication is
    // observed without allowing any serial or hardware access.
    for (matching, action) in [false, true].into_iter().flat_map(|matching| {
        ["inspect", "segment", "authority", "poll"].map(|action| (matching, action))
    }) {
        let (mut s, mut c) = fixture("hang");
        until(&mut s, |s| {
            s.poll().unwrap();
            s.connected()
        });
        let (parent, mut peer) = Channels::pair(s.parent, s.worker).unwrap();
        s.channels = parent;
        s.connected = true;
        s.ready = true;
        let now = SystemClock.now();
        s.last_observed = now.as_micros();
        let deadline = lamp_turn_owner::MonoTime::from_micros(now.as_micros() + 500_000);
        c.set_capture(now, CaptureState::RetainingUntil(deadline))
            .unwrap();
        c.set_admission(now, AdmissionState::OpenUntil(deadline))
            .unwrap();
        let owner = c.admit(now, AdmittedInput::NewTurn).unwrap();
        let camera = c.camera_grant(now).unwrap();
        let output = c
            .plan_output(now, owner, OutputKind::Motion, Some(camera))
            .unwrap();
        let permit = c.issue_output(now, output, 240_000).unwrap();
        let snapshot = c.snapshot(now).unwrap();
        let calibration = UnitCalibration::new(
            UnitId::new("inert").unwrap(),
            Joint::ALL.map(|joint| CalibrationRecord {
                joint,
                id: joint.id() as u16,
                drive_mode: 0,
                homing_offset: 333,
                range_min: 1000,
                range_max: 3000,
            }),
        )
        .unwrap();
        let expected_worker = if matching {
            s.worker
        } else {
            new_boot().unwrap()
        };
        assert_eq!(expected_worker == s.worker, matching);
        let placement_source = new_boot().unwrap();
        let mut q = HeadQualification::new(
            HeadPlan {
                unit: "inert".into(),
                qualification_record: [7; 32],
                expected_initial_counts: [2000; 5],
                minimum_count: 1990,
                maximum_count: 2020,
                outbound_count: 2010,
                segment_duration_us: 80_000,
                maximum_duration_us: 2_000_000,
                readback_age_us: 200_000,
            },
            calibration,
            s.parent,
            expected_worker,
            placement_source,
            now,
        )
        .unwrap();
        q.authority(now, snapshot).unwrap();
        let mut evidence = OriginalEvidence {
            permit,
            placement: wire::Observation {
                source_boot: placement_source,
                frame_sequence: 1,
                acquired_at_us: now.as_micros(),
                valid_until_us: deadline.as_micros(),
                camera,
                qualification_record: [7; 32],
            },
        };
        if action == "segment" {
            q.inspect(now, evidence).unwrap();
            q.observe(
                now,
                &Event::Inspected {
                    request: 1,
                    at_us: now.as_micros(),
                    readback: Readback {
                        joints: [EncoderReadback {
                            counts: 2000,
                            received_at_us: now.as_micros(),
                            velocity_raw: 0,
                            load_magnitude_raw: 0,
                            moving_raw: 0,
                            status_raw: 0,
                        }; 5],
                    },
                },
            )
            .unwrap();
            evidence.placement.frame_sequence = 2;
        }
        let result = match action {
            "inspect" => q.send_inspection(&mut s, now, evidence),
            "segment" => q.send_segment(&mut s, now, evidence),
            "authority" => q.send_authority(&mut s, now, snapshot),
            "poll" => q.poll(&mut s, |_| {}),
            _ => unreachable!(),
        };
        let command = peer
            .data
            .receive::<MotorCommand>(SystemClock.now())
            .unwrap();
        let control = peer.control.receive::<Control>(SystemClock.now()).unwrap();
        let unrelated_was_stopped = s.stop_at.is_some();
        // Explicit test cleanup remains independent from the operation result.
        s.fail(
            "inert qualification identity test cleanup",
            SystemClock.now().as_micros(),
        );
        until(&mut s, |s| {
            let _ = s.poll();
            s.reaped()
        });
        if matching {
            assert!(result.is_ok(), "{action} rejected its original Supervisor");
            assert_ne!(q.phase(), Phase::Inhibited);
            match action {
                "inspect" => assert!(matches!(
                    command,
                    Some(MotorCommand::Inspect { request: 1, .. })
                )),
                "segment" => assert!(
                    matches!(command, Some(MotorCommand::SelectedSegment { request: 2, selection, .. }) if selection.mask() == 16)
                ),
                _ => assert!(command.is_none()),
            }
            assert_eq!(
                matches!(control, Some(Control::Authority { .. })),
                action == "authority"
            );
        } else {
            assert!(
                result.is_err(),
                "{action} accepted a different Supervisor worker"
            );
            assert_eq!(q.phase(), Phase::Inhibited, "{action}");
            assert!(command.is_none(), "{action} sent data to the wrong worker");
            assert!(
                control.is_none(),
                "{action} sent control to the wrong worker"
            );
        }
        assert!(
            !unrelated_was_stopped,
            "{action} stopped the supplied worker"
        );
    }
}
#[test]
fn actual_child_uses_same_worker_loop_and_stop_reaps_before_clean_replacement() {
    let (mut s, mut c) = fixture("normal");
    ready(&mut s, &mut c);
    assert!(!s.reaped());
    assert!(
        s.replacement_allowed(Path::new("/tmp/not-opened"), new_boot().unwrap())
            .is_err()
    );
    s.stop().unwrap();
    let mut events = Vec::new();
    until(&mut s, |s| {
        if let Some(e) = s.poll().unwrap() {
            events.push(e)
        }
        s.reaped()
    });
    // Exit may race unread final packets. Poll the remaining bounded receipts;
    // a child exit alone is never a clean close receipt.
    for _ in 0..8 {
        if let Some(e) = s.poll().unwrap() {
            events.push(e)
        } else {
            break;
        }
    }
    assert!(s.clean_exit(), "{:?}", s.failure());
    assert!(events.iter().any(|e| matches!(e, Event::Inhibited { .. })));
    assert!(events.iter().any(|e| matches!(e, Event::Stopped { .. })));
    assert!(
        s.replacement_allowed(Path::new("/tmp/not-opened"), new_boot().unwrap())
            .is_ok()
    );
    assert!(
        s.replacement_allowed(Path::new("/tmp/not-opened"), s.worker)
            .is_err()
    );
    let old = s.worker;
    drop(s);
    let (mut replacement, mut c) = fixture("normal");
    assert_ne!(old, replacement.worker);
    ready(&mut replacement, &mut c);
    replacement.stop().unwrap();
    until(&mut replacement, |s| {
        s.poll().unwrap();
        s.reaped()
    });
}
#[test]
fn crashed_child_is_reaped_without_a_successful_shutdown_claim() {
    let (mut s, _) = fixture("crash");
    until(&mut s, |s| {
        let _ = s.poll();
        s.reaped()
    });
    assert!(!s.clean_exit());
    assert!(s.failure().is_some());
}
#[test]
fn hung_owned_child_is_killed_and_reaped_after_missing_inhibit() {
    let (mut s, _) = fixture("hang");
    until(&mut s, |s| {
        s.poll().unwrap();
        s.connected()
    });
    s.stop().unwrap();
    // Exercise the real expiry/kill path without making host scheduling a test
    // of the 20 ms target. This does not change the production bound.
    s.stop_at = Some(SystemClock.now().as_micros() - INHIBIT_DEADLINE_US);
    until(&mut s, |s| {
        let _ = s.poll();
        s.reaped()
    });
    assert!(s.killed_at.is_some());
    assert!(!s.clean_exit());
    assert!(s.failure().unwrap().contains("inhibit"));
}
#[test]
fn malformed_peer_cannot_bypass_expired_watch_or_keep_child_alive() {
    let (mut s, _) = fixture("hang");
    until(&mut s, |s| {
        s.poll().unwrap();
        s.connected()
    });
    // Named child already owns endpoints. A separate wrong producer cannot
    // replace it. Inject parser failure through the actual channel pair while
    // retaining this owned child as the supervised process.
    let (parent, mut peer) = Channels::pair(s.parent, s.worker).unwrap();
    s.channels = parent;
    s.connected = true;
    s.ready = true;
    s.last_observed = SystemClock.now().as_micros() - PROGRESS_DEADLINE_US;
    peer.control
        .send(
            &serde_json::json!({"kind":"ready","extra":true}),
            SystemClock.now(),
        )
        .unwrap();
    assert!(s.poll().is_err());
    assert!(s.killed_at.is_some());
    until(&mut s, |s| {
        let _ = s.poll();
        s.reaped()
    });
    assert!(!s.clean_exit());
}
#[test]
fn abrupt_parent_channel_loss_expires_authority_in_real_child() {
    let (mut s, mut c) = fixture("parent_lost");
    ready(&mut s, &mut c);
    // Remove the parent's transport without sending Stop. Child must stop on
    // authority age (or failed feedback), not continue indefinitely.
    let (replacement, _) = Channels::pair(s.parent, s.worker).unwrap();
    s.channels = replacement;
    let deadline = Instant::now() + Duration::from_secs(2);
    let status = loop {
        if let Some(status) = s.child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2))
    };
    s.exit = Some(status);
    assert!(status.success());
    assert!(!s.clean_exit());
}
#[test]
fn identity_and_cleanup_receipts_fail_closed() {
    let (mut s, _) = fixture("hang");
    let at = SystemClock.now().as_micros();
    assert!(
        s.event(
            &Event::Stopped {
                at_us: at,
                cleanup: wire::Cleanup::default()
            },
            at
        )
        .is_err()
    );
    s.fail("fixture stop", at);
    until(&mut s, |s| {
        let _ = s.poll();
        s.reaped()
    });
    assert!(!s.clean_exit());
}

#[test]
fn socket_mode_transition_waits_without_accepting_nonprivate_or_wrong_type_peer() {
    use std::os::unix::{fs::PermissionsExt, net::UnixDatagram};
    let dir = PathBuf::from("/tmp").join(format!("ms-{}", boot_text(new_boot().unwrap())));
    fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
    let p = new_boot().unwrap();
    let w = new_boot().unwrap();
    let mut channels = Channels::bind(&dir, p, w, false).unwrap();
    let c = UnixDatagram::bind(dir.join("motor.c.w")).unwrap();
    let d = UnixDatagram::bind(dir.join("motor.d.w")).unwrap();
    fs::set_permissions(dir.join("motor.c.w"), fs::Permissions::from_mode(0o700)).unwrap();
    fs::set_permissions(dir.join("motor.d.w"), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!channels.connect_step().unwrap());
    fs::set_permissions(dir.join("motor.d.w"), fs::Permissions::from_mode(0o600)).unwrap();
    assert!(channels.connect_step().unwrap());
    drop(channels);
    drop(c);
    drop(d);
    fs::remove_file(dir.join("motor.c.w")).unwrap();
    fs::remove_file(dir.join("motor.d.w")).unwrap();
    let mut channels = Channels::bind(&dir, p, w, false).unwrap();
    fs::write(dir.join("motor.c.w"), b"not a socket").unwrap();
    assert!(channels.connect_step().is_err());
    drop(channels);
    fs::remove_file(dir.join("motor.c.w")).unwrap();
    fs::remove_dir(dir).unwrap();
}

#[test]
fn inert_child_stop_timing_samples_are_host_ipc_only() {
    // A timing sample, not a deadline check. The supervisor keeps its real-clock
    // bounds (progress, inhibit, packet age), so a loaded or paused host can
    // still make it kill a cycle; that cycle is recorded with its reason instead
    // of failing the test. The clean stop itself is asserted by
    // actual_child_uses_same_worker_loop_and_stop_reaps_before_clean_replacement.
    let mut inhibition = Vec::new();
    let mut exit = Vec::new();
    let mut bound_expired = Vec::new();
    for _ in 0..12 {
        let (mut s, mut c) = fixture("normal");
        let mut sent = false;
        until(&mut s, |s| {
            let reading = matches!(s.poll(), Ok(Some(Event::Progress { .. })));
            if reading && !sent && s.failure().is_none() {
                let _ = s.send_control(&Control::Authority {
                    snapshot: c.snapshot(SystemClock.now()).unwrap(),
                });
                sent = true;
            }
            s.is_ready() || s.failure().is_some()
        });
        let requested = SystemClock.now().as_micros();
        let _ = s.stop();
        let mut sample = None;
        // Exit may come before the last receipts are read, so wait for Stopped
        // (or a failure) as well as the reap.
        until(&mut s, |s| {
            if let Ok(Some(Event::Inhibited { report, .. })) = s.poll() {
                sample = Some(report.stopped_at_us - requested)
            }
            s.reaped() && (s.stopped || s.failure().is_some())
        });
        assert!(s.directory.peers_cleaned);
        match s.failure() {
            Some(why) => bound_expired.push(why.to_owned()),
            None => {
                assert!(s.clean_exit());
                inhibition.push(sample.expect("clean stop without an inhibition receipt"));
                exit.push(SystemClock.now().as_micros() - requested);
            }
        }
    }
    println!(
        "motor_inert_timing {}",
        serde_json::json!({"scope":"host_private_unix_subprocess_stop_to_local_inhibition_no_serial_no_motion","samples_us":inhibition,"stop_to_reaped_us":exit,"bound_expired":bound_expired,"runtime_bound_us":INHIBIT_DEADLINE_US,"hardware_qualified":false})
    );
}

#[test]
fn serial_cleanup_failure_and_stalled_teardown_never_become_clean_exit() {
    for bad_cleanup in [true, false] {
        let (mut s, _) = fixture("hang");
        until(&mut s, |s| {
            s.poll().unwrap();
            s.connected()
        });
        let n = SystemClock.now().as_micros();
        s.event(
            &Event::Inhibited {
                at_us: n,
                motion: None,
                report: wire::StopEvidence {
                    profile_preparation: None,
                    privacy_revision: None,
                    reason: "Shutdown".into(),
                    stopped_at_us: n,
                    accepted_request_bytes: 0,
                    partial_request_bytes: 0,
                    position_request: false,
                    discard_error: None,
                },
            },
            n,
        )
        .unwrap();
        if bad_cleanup {
            s.expect_serial = true;
            s.event(
                &Event::Stopped {
                    at_us: n,
                    cleanup: wire::Cleanup {
                        serial_closed: Some(false),
                        settings_restored: Some(false),
                        error: Some("injected close failure".into()),
                    },
                },
                n,
            )
            .unwrap();
            assert!(s.failure().is_some());
        }
        s.inhibited_received = Some(SystemClock.now().as_micros() - TEARDOWN_US);
        until(&mut s, |s| {
            let _ = s.poll();
            s.reaped()
        });
        assert!(s.killed_at.is_some());
        assert!(!s.clean_exit());
    }
}

#[test]
fn audit_failed_privacy_send_inhibits_supervisor_immediately() {
    let (mut s, mut c) = fixture("hang_after_ready");
    ready(&mut s, &mut c);
    c.set_microphone_permission(SystemClock.now(), Permission::Denied)
        .unwrap();
    let snapshot = c.snapshot(SystemClock.now()).unwrap();
    let mut congested = false;
    let mut accepted = 0;
    for _ in 0..4096 {
        match s.send_control(&Control::Authority { snapshot }) {
            Ok(()) => accepted += 1,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                congested = true;
                break;
            }
            Err(e) => panic!("unexpected send failure: {e}"),
        }
    }
    assert!(congested, "fixture never filled the finite control queue");
    let still_ready = s.is_ready();
    let failure_latched = s.failure().is_some();
    let killed = s.killed_at.is_some();
    let command_accepted = s
        .send_command(&MotorCommand::Inspect {
            request: 1,
            requested_at_us: SystemClock.now().as_micros(),
        })
        .is_ok();
    println!(
        "audit_priority_send accepted={accepted} ready={still_ready} failure={failure_latched} killed={killed} command_accepted={command_accepted}"
    );
    let dir = s.directory().to_owned();
    let _ = s.stop();
    until(&mut s, |s| {
        let _ = s.poll();
        s.reaped()
    });
    drop(s);
    let _ = fs::remove_dir_all(dir);
    assert!(
        !still_ready && failure_latched && killed && !command_accepted,
        "failed privacy send left motor ready and open to data commands"
    );
}

#[test]
fn audit_crashed_child_does_not_leak_owned_ipc_directory() {
    let (mut s, _) = fixture("crash");
    let dir = s.directory().to_owned();
    until(&mut s, |s| {
        let _ = s.poll();
        s.reaped()
    });
    drop(s);
    let leftovers = fs::read_dir(&dir)
        .map(|entries| entries.map(|e| e.unwrap().file_name()).collect::<Vec<_>>())
        .unwrap_or_default();
    let leaked = dir.exists();
    println!("audit_crash_directory leaked={leaked} leftovers={leftovers:?}");
    let _ = fs::remove_dir_all(dir);
    assert!(
        !leaked,
        "reaped child leaves per-worker socket paths and directory behind"
    );
}

// Shared fake STS3215 bus: each suite uses only the fixture operations it needs.
#[allow(dead_code)]
#[path = "../../../tests/runtime/support.rs"]
mod core_fixture;

#[test]
fn real_core_fault_receipts_are_accepted_by_supervisor_without_restamping_stop() {
    let mut r = core_fixture::Rig::new();
    r.wire.borrow_mut().profile_speed = 0;
    r.inspect();
    let command = r.segment();
    r.send(&command);
    r.clock.1.set(true);
    assert!(r.step().is_err());
    assert!(r.finish().is_err());
    let (mut s, _) = fixture("hang");
    // The producer uses a deterministic clock. Feed its exact, unmodified
    // receipts through the real supervisor validator on that same time domain.
    s.started = 0;
    s.last_observed = 0;
    s.parent = core_fixture::b(1);
    s.worker = core_fixture::b(2);
    let mut rejected_at = None;
    let mut original_stop = None;
    for event in &r.events {
        s.event(event, event.at_us()).unwrap();
        match event {
            Event::Rejected { at_us, .. } => rejected_at = Some(*at_us),
            Event::Inhibited { at_us, report, .. } => {
                assert_eq!(report.reason, "Profile(BaseYaw)");
                assert!(report.stopped_at_us < rejected_at.unwrap());
                assert!(*at_us >= rejected_at.unwrap());
                original_stop = Some(report.stopped_at_us);
            }
            _ => {}
        }
    }
    assert!(original_stop.is_some());
    assert!(s.inhibited && s.stopped);
    assert!(s.failure().unwrap().contains("Profile(BaseYaw)"));
    assert!(s.killed_at.is_none(), "cooperative fault was rejected");
    assert!(r.wire.borrow().goals.is_empty());
    s.fail("fixture cleanup", SystemClock.now().as_micros());
    until(&mut s, |s| {
        let _ = s.poll();
        s.reaped()
    });
}

#[test]
fn inhibition_measurement_must_be_between_spawn_and_publication() {
    for before_spawn in [true, false] {
        let (mut s, _) = fixture("hang");
        let n = SystemClock.now().as_micros();
        let event = Event::Inhibited {
            at_us: n,
            motion: None,
            report: wire::StopEvidence {
                profile_preparation: None,
                privacy_revision: None,
                reason: "Shutdown".into(),
                stopped_at_us: if before_spawn { s.started - 1 } else { n + 1 },
                accepted_request_bytes: 0,
                partial_request_bytes: 0,
                position_request: false,
                discard_error: None,
            },
        };
        assert!(s.event(&event, n).is_err());
        assert!(!s.inhibited);
        s.fail("fixture cleanup", n);
        until(&mut s, |s| {
            let _ = s.poll();
            s.reaped()
        });
    }
}

#[test]
fn no_peer_cleanup_before_observed_reap_even_after_owned_kill() {
    let (mut s, _) = fixture("hang");
    until(&mut s, |s| {
        s.poll().unwrap();
        s.connected()
    });
    let dir = s.directory().to_owned();
    for name in Directory::PEERS {
        assert!(dir.join(name).exists());
    }
    s.fail("fixture owned kill", SystemClock.now().as_micros());
    assert!(!s.reaped());
    assert!(!s.is_ready());
    assert!(
        s.replacement_allowed(Path::new("/tmp/fresh-unused"), new_boot().unwrap())
            .is_err()
    );
    for name in Directory::PEERS {
        assert!(dir.join(name).exists());
    }
    until(&mut s, |s| {
        let _ = s.poll();
        s.reaped()
    });
    for name in Directory::PEERS {
        assert!(!dir.join(name).exists());
    }
    assert!(s.ipc_cleanup_error().is_none());
    assert!(
        s.replacement_allowed(Path::new("/tmp/fresh-unused"), new_boot().unwrap())
            .is_ok()
    );
    drop(s);
    assert!(!dir.exists());
}

#[test]
fn socket_replacement_is_retained_and_cleanup_failure_blocks_replacement() {
    use std::os::unix::{fs::PermissionsExt, net::UnixDatagram};
    let (mut s, _) = fixture("hang");
    until(&mut s, |s| {
        s.poll().unwrap();
        s.connected()
    });
    let dir = s.directory().to_owned();
    let old = dir.join("old-child-socket");
    let path = dir.join(Directory::PEERS[0]);
    // Keep the old inode allocated, so this does not depend on inode reuse.
    fs::rename(&path, &old).unwrap();
    let replacement = UnixDatagram::bind(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let identity = Identity::of(&fs::symlink_metadata(&path).unwrap());
    s.fail("fixture owned kill", SystemClock.now().as_micros());
    until(&mut s, |s| {
        let _ = s.poll();
        s.reaped()
    });
    assert!(s.ipc_cleanup_error().unwrap().contains("identity changed"));
    assert!(
        s.replacement_allowed(Path::new("/tmp/fresh-unused"), new_boot().unwrap())
            .is_err()
    );
    for _ in 0..3 {
        let _ = s.poll();
    }
    assert!(Identity::of(&fs::symlink_metadata(&path).unwrap()) == identity);
    assert!(!dir.join(Directory::PEERS[1]).exists());
    drop(s);
    assert!(path.exists() && old.exists());
    drop(replacement);
    fs::remove_file(path).unwrap();
    fs::remove_file(old).unwrap();
    fs::remove_dir(dir).unwrap();
}

#[test]
fn replacing_directory_does_not_remove_original_or_replacement_contents() {
    let (mut s, _) = fixture("hang");
    until(&mut s, |s| {
        s.poll().unwrap();
        s.connected()
    });
    let dir = s.directory().to_owned();
    let moved = dir.with_extension("moved");
    fs::rename(&dir, &moved).unwrap();
    fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
    fs::write(dir.join("replacement-owned"), b"retain").unwrap();
    s.fail("fixture owned kill", SystemClock.now().as_micros());
    until(&mut s, |s| {
        let _ = s.poll();
        s.reaped()
    });
    assert!(s.ipc_cleanup_error().unwrap().contains("identity"));
    drop(s);
    assert!(dir.join("replacement-owned").exists());
    for name in ["motor.c.p", "motor.d.p", "motor.c.w", "motor.d.w"] {
        assert!(moved.join(name).exists());
        fs::remove_file(moved.join(name)).unwrap();
    }
    fs::remove_file(dir.join("replacement-owned")).unwrap();
    fs::remove_dir(dir).unwrap();
    fs::remove_dir(moved).unwrap();
}

#[test]
fn partial_child_bind_is_cleaned_after_reap_without_waiting_for_second_socket() {
    let (mut s, _) = fixture("crash_partial");
    let dir = s.directory().to_owned();
    until(&mut s, |s| {
        let _ = s.poll();
        s.reaped()
    });
    assert!(!s.clean_exit());
    assert!(s.ipc_cleanup_error().is_none());
    assert!(s.directory.peers_cleaned);
    assert!(!dir.join("motor.c.w").exists());
    drop(s);
    assert!(!dir.exists());
}

#[test]
fn failed_control_before_connection_revokes_admission_and_retains_owned_child() {
    let (mut s, mut c) = fixture("hang");
    assert!(
        s.send_control(&Control::Authority {
            snapshot: c.snapshot(SystemClock.now()).unwrap(),
        })
        .is_err()
    );
    assert!(s.failure().is_some() && s.killed_at.is_some());
    assert!(!s.is_ready() && !s.reaped());
    assert!(
        s.send_command(&MotorCommand::Inspect {
            request: 1,
            requested_at_us: SystemClock.now().as_micros(),
        })
        .is_err()
    );
    until(&mut s, |s| {
        let _ = s.poll();
        s.reaped()
    });
    assert!(!s.clean_exit());
}

#[test]
fn dropping_reaped_predecessor_does_not_remove_active_successor_sockets() {
    let (mut old, _) = fixture("crash");
    let old_dir = old.directory().to_owned();
    until(&mut old, |s| {
        let _ = s.poll();
        s.reaped()
    });
    let (mut next, mut c) = fixture("normal");
    ready(&mut next, &mut c);
    assert!(
        old.replacement_allowed(next.directory(), next.worker)
            .is_ok()
    );
    let next_dir = next.directory().to_owned();
    drop(old);
    assert!(!old_dir.exists());
    for name in Directory::PEERS {
        assert!(next_dir.join(name).exists());
    }
    assert!(next.is_ready() && !next.reaped());
    // The assertion above is about the predecessor leaving a live successor
    // alone. Fixture cleanup does not make that proof depend on a graceful
    // Stop acknowledgement arriving inside the product's 20 ms deadline.
    // The worker-loop/clean-replacement test checks that separate contract.
    next.fail(
        "successor socket fixture cleanup",
        SystemClock.now().as_micros(),
    );
    until(&mut next, |s| {
        let _ = s.poll();
        s.reaped()
    });
    assert!(next.killed_at.is_some());
    assert!(!next.clean_exit());
    assert!(next.ipc_cleanup_error().is_none());
    assert!(next.directory.peers_cleaned);
    for name in Directory::PEERS {
        assert!(!next_dir.join(name).exists());
    }
    drop(next);
    assert!(!next_dir.exists());
}
