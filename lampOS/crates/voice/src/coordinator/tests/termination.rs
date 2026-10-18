//! Actual process signals with synthetic workers/PCM only. No hardware or cloud.
use super::*;
use crate::{diagnostic_control::DiagnosticStarted, diagnostics::*};
use std::{
    io::{BufRead, BufReader},
    process::{Child, Stdio},
    sync::mpsc,
    thread,
};

const CHILD: &str = "coordinator::tests::termination::isolated_signal_child";
const MODE: &str = "LAMP_DIRECTED_TERMINATION_TEST_MODE";
const OUTPUT: &str = "LAMP_DIRECTED_TERMINATION_TEST_OUTPUT";

struct Reap(Child);
impl Drop for Reap {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn real_signal(signal: &str, mode: &str) {
    let directory = SessionDirectory::create().unwrap();
    let output = directory.path.join("trace");
    fs::create_dir(&output).unwrap();
    fs::set_permissions(&output, fs::Permissions::from_mode(0o700)).unwrap();
    let mut child = Reap(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", CHILD, "--nocapture", "--test-threads=1"])
            .env(MODE, mode)
            .env(OUTPUT, &output)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let stdout = child.0.stdout.take().unwrap();
    let (ready, received) = mpsc::channel();
    let reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let line = line.unwrap();
            if line.contains("LAMP_SIGNAL_READY") {
                let _ = ready.send(());
            }
        }
    });
    received
        .recv_timeout(Duration::from_secs(5))
        .expect("child did not install signal action");
    let sent = Instant::now();
    let sent_at_us = monotonic_us();
    // Use the existing safe syscall wrapper: minimal Linux images need not
    // provide an external /bin/kill utility. This still sends a real OS signal.
    let pid = rustix::process::Pid::from_raw(child.0.id().try_into().unwrap()).unwrap();
    let number = match signal {
        "-INT" => rustix::process::Signal::INT,
        "-TERM" => rustix::process::Signal::TERM,
        _ => panic!("unsupported test signal"),
    };
    rustix::process::kill_process(pid, number).unwrap();
    let deadline = Instant::now() + Duration::from_secs(4);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "signal child did not finish");
        thread::sleep(Duration::from_millis(2));
    };
    reader.join().unwrap();
    assert!(status.success(), "child assertions failed: {status}");
    let rows: Vec<Value> = if mode.starts_with("serve") {
        let mut rows: Vec<Value> = (0..4)
            .flat_map(|slot| {
                fs::read_to_string(output.join(format!("metadata/events-{slot}.jsonl")))
                    .unwrap()
                    .lines()
                    .map(|line| serde_json::from_str::<Value>(line).unwrap())
                    .collect::<Vec<_>>()
            })
            .filter(|row| row["service_sequence"].as_u64().is_some())
            .collect();
        rows.sort_by_key(|row| row["service_sequence"].as_u64().unwrap());
        rows
    } else {
        fs::read_to_string(output.join("events.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    };
    let end = rows.last().unwrap();
    assert_eq!(
        end["status"],
        if mode.starts_with("serve") {
            "stopped_unscored"
        } else {
            "failed"
        }
    );
    assert_eq!(
        end["termination_signal"],
        if signal == "-TERM" {
            "SIGTERM"
        } else {
            "SIGINT"
        }
    );
    if mode.starts_with("serve") {
        assert!(end["error"].is_null());
    } else {
        assert!(end["error"].as_str().unwrap().contains("interrupted by"));
    }
    assert!(!rows.iter().any(|row| row["status"] == "completed_unscored"));
    let observed = rows
        .iter()
        .find(|row| row["kind"] == "synthetic_signal_observed")
        .unwrap();
    let observation_us = observed["at_us"]
        .as_u64()
        .unwrap()
        .checked_sub(sent_at_us)
        .unwrap();
    if mode == "conversation" {
        assert!(
            rows.iter()
                .any(|row| row["kind"] == "turn_finished" && row["outcome"] == "runtime_failed")
        );
        let finished: Vec<_> = rows
            .iter()
            .filter(|row| row["kind"] == "audio_diagnostics_certified")
            .collect();
        assert_eq!(finished.len(), 2);
        assert!(
            finished
                .iter()
                .all(|row| row["valid"] == true && row["receipt"]["acknowledged"] == true)
        );
        for name in ["capture-audio", "render-audio"] {
            assert!(output.join(name).join("complete.json").is_file());
        }
    }
    if mode == "serve" {
        assert!(
            rows.iter()
                .any(|row| row["kind"] == "turn_finished" && row["outcome"] == "service_stopped")
        );
        assert!(
            !rows
                .iter()
                .any(|row| row["kind"] == "worker_forced_shutdown")
        );
    }
    println!(
        "{signal} {mode}: signal syscall invocation to cooperative observation {observation_us} us; terminal trace retained in {:.3}s",
        sent.elapsed().as_secs_f64()
    );
    // These directories contain only this test's generated zero PCM and metadata.
    fs::remove_dir_all(&output).unwrap();
}

#[test]
fn sigterm_retains_failure_and_acknowledged_synthetic_diagnostics() {
    real_signal("-TERM", "conversation");
}
#[test]
fn sigint_retains_failure_and_acknowledged_synthetic_diagnostics() {
    real_signal("-INT", "conversation");
}
#[test]
fn signal_during_startup_pulse_returns_interrupted_and_retains_failed_trace() {
    real_signal("-TERM", "startup");
}

#[test]
fn serve_sigterm_revokes_then_stops_and_reaps_workers_before_logging_finish() {
    real_signal("-TERM", "serve");
}

// Runs only in a separate test process, never installing process-global handlers
// in the normal test runner. A successful child means its failure-path assertions passed.
#[test]
fn isolated_signal_child() {
    let Ok(mode) = std::env::var(MODE) else {
        return;
    };
    let output = PathBuf::from(std::env::var(OUTPUT).unwrap());
    let termination = Termination::install().unwrap();
    let (mut owner, current, _) = active_reply();
    let mut trace = SessionState::from(vec![
        json!({"kind":"run_start","at_us":monotonic_us(),"scope":"synthetic_signal_test"}),
    ]);
    if mode.starts_with("serve") {
        trace = SessionState::service(&output);
        record(
            &mut trace,
            json!({"kind":"run_start","at_us":monotonic_us(),"scope":"synthetic_service_signal"}),
        )
        .unwrap();
        if mode == "serve-muted" {
            return muted_signal_child(output, termination, trace);
        }
        return service_signal_child(output, termination, owner, current, trace);
    }
    let mut cues = None;
    if mode == "startup" {
        println!("LAMP_SIGNAL_READY");
        io::stdout().flush().unwrap();
        let result = loop {
            if let Err(error) = pulse(
                &mut owner,
                &mut [],
                &mut 0,
                &mut StartupEvents::default(),
                &termination,
            ) {
                assert_eq!(error.kind(), io::ErrorKind::Interrupted);
                trace.push(json!({"kind":"synthetic_signal_observed","at_us":monotonic_us()}));
                break Err(error.into());
            }
            thread::sleep(Duration::from_millis(2));
        };
        assert!(finish_run(&output, &mut trace, &mut cues, result, &termination).is_err());
        return;
    }
    let dir = SessionDirectory::create().unwrap();
    let boot = owner.snapshot(now()).unwrap().boot();
    let (mut capture, capture_peer) = Worker::sleeping_fixture(&dir.path, "capture");
    let (mut speaker, speaker_peer) = Worker::sleeping_fixture(&dir.path, "speaker");
    let (provider, provider_peer) = Worker::sleeping_fixture(&dir.path, "provider");
    let (privacy, privacy_peer) = Worker::sleeping_fixture(&dir.path, "privacy");
    let capture_recorder = synthetic_recorder(&output, boot, StreamKind::Capture);
    let render_recorder = synthetic_recorder(&output, boot, StreamKind::Render);
    let capture_start = DiagnosticStarted {
        boot,
        stream: DiagnosticStream::Capture,
        started_at_us: capture_recorder.started_at_us(),
    };
    let render_start = DiagnosticStarted {
        boot,
        stream: DiagnosticStream::Render,
        started_at_us: render_recorder.started_at_us(),
    };
    capture.diagnostic_start = Some(capture_start);
    speaker.diagnostic_start = Some(render_start);
    let peers = [
        peer(capture_peer, Some((capture_recorder, capture_start))),
        peer(speaker_peer, Some((render_recorder, render_start))),
        peer(provider_peer, None),
        peer(privacy_peer, None),
    ];
    let mut rig = Rig {
        second: None,
        microphone: crate::options::Microphone::Mic2,
        end_silence_blocks: None,
        early_end_blocks: None,
        faint_level: None,
        camera: None,
        motor: None,
        provider,
        capture,
        speaker,
        privacy,
        privacy_controls: false,
        ring: None.into(),
        ring_channel_ceiling: None,
        diagnostic_directory: Some(output.clone()),
        noise_suppression: NoiseSuppression::On,
        turn_taking: TurnTaking::VadOnly,
        playout_start: PlayoutStart::default(),
        fixture_at_ms: None,
        endpoint: None,
        call_wait_blocks: None,
        cut_keeps_name: false,
        asker_first: crate::asker_first::Mode::Off,
        follows_on: false,
        paused: Box::new(|| false),
    };
    let mut reply = Some(current);
    println!("LAMP_SIGNAL_READY");
    io::stdout().flush().unwrap();
    let result = conversation(
        &mut rig,
        &mut owner,
        Lifetime::Finite(10),
        &mut trace,
        StartupEvents::default(),
        &mut reply,
        &mut cues,
        &termination,
        &FailureNotices::none(),
    );
    assert_eq!(
        result
            .as_ref()
            .unwrap_err()
            .downcast_ref::<io::Error>()
            .unwrap()
            .kind(),
        io::ErrorKind::Interrupted,
        "{result:?}"
    );
    trace.push(json!({"kind":"synthetic_signal_observed","at_us":monotonic_us()}));
    let result = finish_active_run(
        &mut rig, &mut owner, &mut reply, &mut trace, &mut cues, result,
    );
    assert!(reply.is_none());
    assert_eq!(
        owner.snapshot(now()).unwrap().microphone_permission(),
        Permission::Denied
    );
    for peer in peers {
        assert!(peer.join().unwrap(), "privacy must precede Stop");
    }
    // The synthetic sleep children deliberately need forced reaping. That failure
    // remains in the trace; it does not erase real Recorder finish ACK evidence.
    assert!(
        trace
            .iter()
            .any(|row| row["kind"] == "worker_forced_shutdown")
    );
    assert!(finish_run(&output, &mut trace, &mut cues, result, &termination).is_err());
}

fn synthetic_recorder(output: &Path, boot: BootId, stream: StreamKind) -> Recorder {
    let name = if stream == StreamKind::Capture {
        "capture-audio"
    } else {
        "render-audio"
    };
    let mut recorder = Recorder::start(Config::new(output.join(name), boot, stream)).unwrap();
    let at = monotonic_us();
    assert_eq!(
        recorder.try_privacy(PrivacyMeta {
            at_us: at,
            generation: 2,
            open: true
        }),
        SubmitResult::Queued
    );
    if stream == StreamKind::Capture {
        let meta = CaptureMeta {
            epoch: 1,
            dsp_epoch: 1,
            privacy_generation: 2,
            frame_sequence: 1,
            first_read_started_at_us: at,
            last_read_started_at_us: at,
            read_completed_at_us: at,
            successful_reads: 1,
            processing_completed_at_us: at,
            ..CaptureMeta::default()
        };
        assert_eq!(
            recorder.try_capture(meta, &[0; 160], &[0; 160]),
            SubmitResult::Queued
        );
    } else {
        let meta = RenderMeta {
            playback_epoch: 1,
            privacy_generation: 2,
            first_sample: 0,
            end_sample: 240,
            accepted_at_us: at,
            queue_observed_at_us: at,
            queued_frames: 240,
            ..RenderMeta::default()
        };
        assert_eq!(
            recorder.try_silence(meta, SilenceKind::Prime, 240),
            SubmitResult::Queued
        );
    }
    recorder
}

fn peer(
    channels: WorkerChannels,
    recorder: Option<(Recorder, DiagnosticStarted)>,
) -> thread::JoinHandle<bool> {
    thread::spawn(move || {
        // Retain both endpoints. Capturing only `.control` would close the
        // separate data endpoint before the production loop can poll it.
        let mut channels = channels;
        let mut denied = false;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match channels.control.receive::<Control>().unwrap() {
                Some(Control::Authority { snapshot }) => {
                    denied |= snapshot.microphone_permission() == Permission::Denied
                }
                Some(Control::Stop) => break,
                Some(_) => panic!("unexpected synthetic peer command"),
                None => {}
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        if let Some((mut recorder, started)) = recorder {
            assert!(denied);
            assert_eq!(
                recorder.try_privacy(PrivacyMeta {
                    at_us: monotonic_us(),
                    generation: 3,
                    open: false
                }),
                SubmitResult::Queued
            );
            // This peer stands in for the worker, so the worker's 500 ms
            // (`DEFAULT_FINISH_WAIT`) is not what is tested here: a loaded
            // host's disk took longer, 8 of 72 runs 12 at a time. It waits
            // the most a recorder takes (#1569).
            let report = recorder.finish(
                EndMeta {
                    at_us: monotonic_us(),
                    reason: EndReason::PrivacyClosed,
                },
                crate::diagnostics::MAX_FINISH_WAIT,
            );
            assert!(report.valid, "{report:?}");
            channels
                .control
                .send(WorkerEvent::DiagnosticsFinished {
                    receipt: DiagnosticFinished::from_report(started, report),
                })
                .unwrap();
        }
        denied
    })
}

fn service_signal_child(
    output: PathBuf,
    termination: Termination,
    mut owner: Controller,
    current: Reply,
    mut trace: SessionState,
) {
    let dir = SessionDirectory::create().unwrap();
    let (mut capture, capture_peer, capture_input) =
        Worker::cooperative_fixture(&dir.path, "capture");
    let (mut speaker, speaker_peer, speaker_input) =
        Worker::cooperative_fixture(&dir.path, "speaker");
    let (mut provider, provider_peer, provider_input) =
        Worker::cooperative_fixture(&dir.path, "provider");
    let (mut privacy, privacy_peer, privacy_input) =
        Worker::cooperative_fixture(&dir.path, "privacy");
    capture.end_on_stop(capture_input);
    speaker.end_on_stop(speaker_input);
    provider.end_on_stop(provider_input);
    privacy.end_on_stop(privacy_input);
    let peers = [capture_peer, speaker_peer, provider_peer, privacy_peer].map(|channels| {
        thread::spawn(move || {
            let mut channels = channels;
            let mut denied = false;
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match channels.control.receive::<Control>().unwrap() {
                    Some(Control::Authority { snapshot }) => {
                        denied |= snapshot.microphone_permission() == Permission::Denied
                    }
                    Some(Control::Stop) => break,
                    Some(_) => panic!("unexpected synthetic control"),
                    None => {}
                }
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(1));
            }
            // Keep the host mock endpoints alive through the supervisor's final
            // drain. macOS datagram close reports reset differently from Linux;
            // native worker socket teardown still needs target qualification.
            (denied, channels)
        })
    });
    let mut rig = Rig {
        second: None,
        microphone: crate::options::Microphone::Mic2,
        end_silence_blocks: None,
        early_end_blocks: None,
        faint_level: None,
        camera: None,
        motor: None,
        provider,
        capture,
        speaker,
        privacy,
        privacy_controls: false,
        ring: None.into(),
        ring_channel_ceiling: None,
        diagnostic_directory: None,
        noise_suppression: NoiseSuppression::On,
        turn_taking: TurnTaking::VadOnly,
        playout_start: PlayoutStart::default(),
        fixture_at_ms: None,
        endpoint: None,
        call_wait_blocks: None,
        cut_keeps_name: false,
        asker_first: crate::asker_first::Mode::Off,
        follows_on: false,
        paused: Box::new(|| false),
    };
    let mut reply = Some(current);
    let mut cues = None;
    println!("LAMP_SIGNAL_READY");
    io::stdout().flush().unwrap();
    let result = conversation(
        &mut rig,
        &mut owner,
        Lifetime::Serve,
        &mut trace,
        StartupEvents::default(),
        &mut reply,
        &mut cues,
        &termination,
        &FailureNotices::none(),
    );
    assert!(result.is_ok(), "{result:?}");
    record(
        &mut trace,
        json!({"kind":"synthetic_signal_observed","at_us":monotonic_us()}),
    )
    .unwrap();
    let result = finish_active_run(
        &mut rig, &mut owner, &mut reply, &mut trace, &mut cues, result,
    );
    assert!(result.is_ok(), "{result:?}");
    assert!(reply.is_none());
    assert_eq!(
        owner.snapshot(now()).unwrap().microphone_permission(),
        Permission::Denied
    );
    for peer in peers {
        assert!(peer.join().unwrap().0, "privacy must precede Stop");
    }
    assert!(finish_run(&output, &mut trace, &mut cues, result, &termination).is_ok());
}

#[test]
fn sigterm_while_muted_reaps_only_privacy_without_opening_audio() {
    real_signal("-TERM", "serve-muted");
}

fn muted_signal_child(output: PathBuf, termination: Termination, mut trace: SessionState) {
    use crate::service_privacy::{
        self,
        tests::{Script, mock},
    };
    use std::sync::{Arc, atomic::AtomicBool};
    let directory = SessionDirectory::create().unwrap();
    let (worker, peer) = mock(
        &directory.path,
        Script::Muted,
        Arc::new(AtomicBool::new(false)),
    );
    let mut worker = Some(worker);
    let mut announced = false;
    let result = crate::coordinator::service::supervise(
        &mut trace,
        &mut None,
        &termination,
        |trace, cues| {
            service_privacy::wait_worker(
                worker.take().expect("no second wait after signal"),
                new_boot().unwrap(),
                &termination,
                |state, _, _| {
                    if state == PrivacyState::Muted && !announced {
                        announced = true;
                        println!("LAMP_SIGNAL_READY");
                        io::stdout().flush().unwrap();
                    }
                    record(
                        trace,
                        json!({"kind":"test_privacy_state","at_us":monotonic_us(),"state":format!("{state:?}")}),
                    )?;
                    flush_cues(trace, cues);
                    Ok(())
                },
            )
        },
        |_, _| panic!("microphone/provider startup while muted"),
    );
    assert!(result.is_ok(), "{result:?}");
    assert!(announced);
    assert!(peer.join().unwrap().received_stop);
    record(
        &mut trace,
        json!({"kind":"synthetic_signal_observed","at_us":monotonic_us()}),
    )
    .unwrap();
    assert!(finish_run(&output, &mut trace, &mut None, result, &termination).is_ok());
    fs::remove_dir(&directory.path).unwrap();
}

/// #960: only a stop the service was asked for releases the restart guard
/// when its cleanup reports a fault; a fault or a privacy exit never does.
#[test]
fn only_a_requested_stop_releases_the_mute_guard_despite_a_cleanup_fault() {
    // Asked to stop (systemd's SIGTERM on an update), ended cleanly or at a
    // termination check: released, whatever the cleanup then says.
    assert!(guard_released_on_stop(true, Some("SIGTERM")));
    assert!(guard_released_on_stop(true, Some("SIGINT")));
    // A fault or a privacy exit while a signal also arrived: kept.
    assert!(!guard_released_on_stop(false, Some("SIGTERM")));
    // No signal: a crash or a fault, kept as before (a clean cycle releases
    // it on its own path, `result.is_ok()`).
    assert!(!guard_released_on_stop(true, None));
    assert!(!guard_released_on_stop(false, None));
}

/// #1567, A: a fault the conversation returned once the stop's signal had
/// come is the stop's; one returned before it, a privacy exit, and a clean
/// end are not this rule's.
#[test]
fn a_fault_after_the_stops_signal_is_the_stops() {
    assert!(stop_before_fault(true, Some("SIGTERM")));
    assert!(stop_before_fault(true, Some("SIGINT")));
    assert!(!stop_before_fault(true, None), "no signal yet: a fault");
    assert!(!stop_before_fault(false, Some("SIGTERM")), "not a fault");
}

/// #1567, B, through the cycle's own wrapper: a run that failed before it
/// heard anything releases the guard and says so in the trace, the error
/// returned as it was; a privacy exit or a run that heard keeps the guard.
#[test]
fn a_cycle_that_failed_before_it_heard_releases_the_guard() {
    use crate::service_privacy::{PhysicalMute, SoftwareMute};
    use std::os::unix::fs::PermissionsExt;
    let directory = std::env::temp_dir().join(format!("lv-guard-{}", monotonic_us()));
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    let record = directory.join("mute.json");
    let muted_on_disk = || {
        lamp_controls::mute::MuteRecord::load(&record, "test-hand-clock", monotonic_us())
            .unwrap()
            .muted
    };
    let fresh = || {
        let mut controls =
            HandControls::with_test_guard("test-hand-clock", monotonic_us(), &record).unwrap();
        controls.arm_active().unwrap();
        Some(controls)
    };
    let failed = || -> Result<()> {
        Err(io::Error::other("capture: snd_pcm_open failed with errno 16").into())
    };

    let mut trace = SessionState::default();
    let mut controls = fresh();
    assert!(muted_on_disk());
    let error = released_if_never_listened(failed(), &mut trace, &mut controls).unwrap_err();
    assert!(error.to_string().contains("errno 16"));
    assert!(!muted_on_disk(), "released: the latch as it was");
    assert!(
        trace.iter().any(
            |row| row["kind"] == "guard_released_never_listened" && row["could_not_start"] == 1
        )
    );

    for exit in [
        Box::new(PhysicalMute { observed_at_us: 1 }) as Box<dyn std::error::Error + Send + Sync>,
        Box::new(SoftwareMute {
            observed_at_us: 1,
            asleep: false,
        }),
    ] {
        let mut controls = fresh();
        assert!(
            released_if_never_listened(Err(exit), &mut SessionState::default(), &mut controls)
                .is_err()
        );
        assert!(muted_on_disk(), "a privacy exit keeps the guard");
    }

    let mut controls = fresh();
    controls.as_mut().unwrap().heard_audio();
    assert!(
        released_if_never_listened(failed(), &mut SessionState::default(), &mut controls).is_err()
    );
    assert!(muted_on_disk(), "it heard, so the guard stands");
    assert!(
        released_if_never_listened(Ok(()), &mut SessionState::default(), &mut controls).is_ok()
    );
    let _ = fs::remove_dir_all(&directory);
}

/// #1567, A with its gates (the reviewer on #1592): off unless set, never
/// with a control input queued and unread, and only for a fault after the
/// signal.
#[test]
fn the_stop_first_rule_is_off_unless_set_and_never_with_a_press_in_flight() {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::Ordering;
    let directory = std::env::temp_dir().join(format!("lv-stopfirst-{}", monotonic_us()));
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    let record = directory.join("mute.json");
    let on = HandControls::with_test_guard("test-hand-clock", monotonic_us(), &record).unwrap();
    assert!(released_by_stop_first(Some(&on), true, Some("SIGTERM")));
    assert!(!released_by_stop_first(Some(&on), true, None), "no signal");
    assert!(
        !released_by_stop_first(Some(&on), false, Some("SIGTERM")),
        "a mute, or no fault"
    );
    assert!(
        !released_by_stop_first(None, true, Some("SIGTERM")),
        "no controls"
    );
    on.queued_for_test().fetch_add(1, Ordering::AcqRel);
    assert!(
        !released_by_stop_first(Some(&on), true, Some("SIGTERM")),
        "a press in flight keeps the guard"
    );
    let mut off =
        HandControls::with_test_guard("test-hand-clock", monotonic_us(), &record).unwrap();
    off.set_never_listened_for_test(false);
    assert!(
        !released_by_stop_first(Some(&off), true, Some("SIGTERM")),
        "off unless set"
    );
    let _ = fs::remove_dir_all(&directory);
}

/// Jarvis's P2 on #1592: when the stop-first rule (A) is the release's only
/// reason, a control that came during the teardown, queued or taken and
/// unsaved, keeps the guard; with nothing in flight it releases. The clean
/// stop's own rule (#960) is unchanged.
#[test]
fn a_press_during_the_stops_teardown_keeps_the_guard() {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::Ordering;
    let directory = std::env::temp_dir().join(format!("lv-teardown-{}", monotonic_us()));
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    let record = directory.join("mute.json");
    let muted = || {
        lamp_controls::mute::MuteRecord::load(&record, "test-hand-clock", monotonic_us())
            .unwrap()
            .muted
    };
    let armed = || {
        let mut controls =
            HandControls::with_test_guard("test-hand-clock", monotonic_us(), &record).unwrap();
        controls.arm_active().unwrap();
        controls
    };
    // Classified with nothing in flight; a press queued in the teardown.
    let mut controls = armed();
    controls.queued_for_test().fetch_add(1, Ordering::AcqRel);
    assert!(!released_after_cleanup(&mut controls, true).unwrap());
    assert!(muted(), "the press keeps the guard");
    // A press taken and not yet saved, the same.
    let mut controls = armed();
    controls.set_dirty_for_test();
    assert!(!released_after_cleanup(&mut controls, true).unwrap());
    assert!(muted());
    // Nothing came: released, the latch as it was.
    let mut controls = armed();
    assert!(released_after_cleanup(&mut controls, true).unwrap());
    assert!(!muted());
    // The clean stop's rule is not A's: it releases as before (#960).
    let mut controls = armed();
    controls.queued_for_test().fetch_add(1, Ordering::AcqRel);
    assert!(released_after_cleanup(&mut controls, false).unwrap());
    let _ = fs::remove_dir_all(&directory);
}

/// The reviewer's P3s on #1592: the two places a run is marked as having
/// heard, through the helpers the cycle and the conversation use, keep the
/// guard for its fault; without them the same fault releases it.
#[test]
fn a_frame_or_a_spoken_startup_marks_the_run_heard() {
    use std::os::unix::fs::PermissionsExt;
    let directory = std::env::temp_dir().join(format!("lv-heard-{}", monotonic_us()));
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    let record = directory.join("mute.json");
    let muted = || {
        lamp_controls::mute::MuteRecord::load(&record, "test-hand-clock", monotonic_us())
            .unwrap()
            .muted
    };
    let armed = || {
        let mut controls =
            HandControls::with_test_guard("test-hand-clock", monotonic_us(), &record).unwrap();
        controls.arm_active().unwrap();
        Some(controls)
    };
    let failed = || -> Result<()> { Err(io::Error::other("a fault").into()) };
    let spoke = StartupEvents {
        microphone_spoke: true,
        ..StartupEvents::default()
    };
    let mut controls = armed();
    heard_if_spoke(&spoke, &mut controls);
    let _ = released_if_never_listened(failed(), &mut SessionState::default(), &mut controls);
    assert!(muted(), "spoke before activation: heard");
    let mut controls = armed();
    heard_a_frame(&mut controls);
    let _ = released_if_never_listened(failed(), &mut SessionState::default(), &mut controls);
    assert!(muted(), "a frame: heard");
    let mut controls = armed();
    heard_if_spoke(&StartupEvents::default(), &mut controls);
    let _ = released_if_never_listened(failed(), &mut SessionState::default(), &mut controls);
    assert!(!muted(), "nothing spoke, no frame: released");
    let _ = fs::remove_dir_all(&directory);
}
