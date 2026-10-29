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
    // Each mode takes its reply only once its workers are up, just before it
    // says it is ready: [`active_reply`]'s capture and admission last 500 ms
    // of the real clock, and starting four fixture processes took 312 to
    // 587 ms with the module's tests in parallel. Taken first, a run whose
    // start took 484 ms or more found its turn gone and ended `StaleOwner`
    // (6 of 20; #1569 row 22).
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
        // The trace's writer, written once before the run is said ready. In
        // this re-executed test binary, with the module's tests in parallel,
        // its first write came late (563 ms in one run, its later ones in
        // microseconds), and a stop soon after found the stop's 500 ms finish
        // wait over with the run's rows still queued: the trace held only
        // `run_start`, never `run_end`. 8 of 15 runs of `termination::`
        // failed so, and none of 15 with this (#1569 row 30).
        trace.flush(&mut None, monotonic_us());
        let written_by = Instant::now() + Duration::from_secs(5);
        while trace.written_events() == 0 {
            assert!(
                Instant::now() < written_by,
                "the trace's writer never wrote"
            );
            thread::sleep(Duration::from_millis(1));
        }
        if mode == "serve-muted" {
            return muted_signal_child(output, termination, trace);
        }
        return service_signal_child(output, termination, trace);
    }
    let mut cues = None;
    if mode == "startup" {
        let (mut owner, _, _) = active_reply();
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
    let (mut capture, capture_peer) = Worker::sleeping_fixture(&dir.path, "capture");
    let (mut speaker, speaker_peer) = Worker::sleeping_fixture(&dir.path, "speaker");
    let (provider, provider_peer) = Worker::sleeping_fixture(&dir.path, "provider");
    let (privacy, privacy_peer) = Worker::sleeping_fixture(&dir.path, "privacy");
    let (mut owner, current, _) = active_reply();
    let boot = owner.snapshot(now()).unwrap().boot();
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
        expression_out: None,
        second: None,
        microphone: crate::options::Microphone::Mic2,
        end_silence_blocks: None,
        early_end_blocks: None,
        early_finalize: false,
        open_name: false,
        call_takes_next: false,
        unfinished_wait: false,
        faint_level: None,
        camera: None,
        motor: None,
        provider,
        capture,
        speaker,
        privacy,
        privacy_controls: false,
        capture_held: false,
        privacy_receipts: false,
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

/// As [`peer`], for #1620: on its Stop this worker's side waits `late`, then
/// finishes its recording, says so and closes its child's `input`, as a
/// worker that takes that long to stop.
fn late_peer(
    channels: WorkerChannels,
    recorder: Option<(Recorder, DiagnosticStarted)>,
    late: Duration,
    input: Option<std::process::ChildStdin>,
) -> thread::JoinHandle<WorkerChannels> {
    thread::spawn(move || {
        let mut channels = channels;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match channels.control.receive::<Control>() {
                Ok(Some(Control::Stop)) => break,
                Ok(_) => {}
                Err(_) => return channels,
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        thread::sleep(late);
        if let Some((mut recorder, started)) = recorder {
            let _ = recorder.try_privacy(PrivacyMeta {
                at_us: monotonic_us(),
                generation: 3,
                open: false,
            });
            let report = recorder.finish(
                EndMeta {
                    at_us: monotonic_us(),
                    reason: EndReason::PrivacyClosed,
                },
                crate::diagnostics::MAX_FINISH_WAIT,
            );
            let _ = channels.control.send(WorkerEvent::DiagnosticsFinished {
                receipt: DiagnosticFinished::from_report(started, report),
            });
        }
        drop(input);
        // Retain both sockets in the join result until shutdown has completed
        // its final drain, independently of the fixture child's exit.
        channels
    })
}

/// #1620, for Seventh (the first builder's 00:56 on #705): at a fixture's
/// finish, a worker that takes longer than the shared 750 ms
/// (`DIAGNOSTIC_SHUTDOWN_GRACE`) to stop. Here the speaker's side finishes
/// its recording 900 ms after its Stop, and the others at once.
///
/// What the run says must name what held the stop, and the microphone must
/// be closed after. The failed diagnostic finish must not replace the
/// speaker's forced-stop reason.
#[test]
fn a_speaker_that_misses_the_finish_deadline_is_named_and_the_microphone_is_closed() {
    let (said, forced) = speaker_finish_failure(true, false);
    assert_eq!(forced, [json!("speaker")], "{forced:?}");
    assert!(said.contains("speaker"), "{said}");
    assert!(said.contains("shared 750 ms deadline"), "{said}");
    assert!(said.contains("forced termination required"), "{said}");
}

#[test]
fn missing_speaker_diagnostics_still_fail_after_clean_worker_exits() {
    let (said, forced) = speaker_finish_failure(false, false);
    assert!(forced.is_empty(), "{forced:?}");
    assert!(said.contains("diagnostic finish"), "{said}");
}

#[test]
fn both_late_audio_workers_keep_their_cleanup_causes() {
    let (said, forced) = speaker_finish_failure(true, true);
    assert_eq!(forced, [json!("speaker"), json!("capture")], "{forced:?}");
    for worker in ["speaker", "capture"] {
        assert!(
            said.contains(&format!(
                "{worker} worker did not stop within shared 750 ms deadline"
            )),
            "{said}"
        );
    }
    assert_eq!(
        said.matches("forced termination required").count(),
        2,
        "{said}"
    );
    assert!(said.contains("diagnostic finish"), "{said}");
    assert!(said.contains("speaker Certification:"), "{said}");
    assert!(said.contains("capture Certification:"), "{said}");
}

fn speaker_finish_failure(speaker_late: bool, capture_late: bool) -> (String, Vec<Value>) {
    use std::os::unix::fs::PermissionsExt;
    // One folder each: the module's three callers run in parallel, and two
    // named by the microsecond alone once found the same name taken
    // (`File exists`, seen 3 times in 45 runs of `termination::`; #1569 row 32).
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let output = std::env::temp_dir().join(format!(
        "lv-1620-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir(&output).unwrap();
    fs::set_permissions(&output, fs::Permissions::from_mode(0o700)).unwrap();
    let (mut owner, current, _) = active_reply();
    let boot = owner.snapshot(now()).unwrap().boot();
    let dir = SessionDirectory::create().unwrap();
    let (mut capture, capture_peer, capture_input) =
        Worker::cooperative_fixture(&dir.path, "capture");
    let (mut speaker, speaker_peer, speaker_input) =
        Worker::cooperative_fixture(&dir.path, "speaker");
    let (mut provider, provider_peer, provider_input) =
        Worker::cooperative_fixture(&dir.path, "provider");
    let (mut privacy, privacy_peer, privacy_input) =
        Worker::cooperative_fixture(&dir.path, "privacy");
    provider.end_on_stop(provider_input);
    privacy.end_on_stop(privacy_input);
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
        late_peer(
            capture_peer,
            Some((capture_recorder, capture_start)),
            if capture_late {
                Duration::from_millis(900)
            } else {
                Duration::ZERO
            },
            Some(capture_input),
        ),
        late_peer(
            speaker_peer,
            speaker_late.then_some((render_recorder, render_start)),
            if speaker_late {
                Duration::from_millis(900)
            } else {
                Duration::ZERO
            },
            Some(speaker_input),
        ),
        late_peer(provider_peer, None, Duration::ZERO, None),
        late_peer(privacy_peer, None, Duration::ZERO, None),
    ];
    let mut rig = Rig {
        expression_out: None,
        second: None,
        microphone: crate::options::Microphone::Mic2,
        end_silence_blocks: None,
        early_end_blocks: None,
        early_finalize: false,
        open_name: false,
        call_takes_next: false,
        unfinished_wait: false,
        faint_level: None,
        camera: None,
        motor: None,
        provider,
        capture,
        speaker,
        privacy,
        privacy_controls: false,
        capture_held: false,
        privacy_receipts: false,
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
    let mut trace = SessionState::default();
    // The fixture's run, ended by its stop as a SIGTERM ends one.
    let stopped: Result<()> = Err(io::Error::new(
        io::ErrorKind::Interrupted,
        "directed run interrupted by SIGTERM",
    )
    .into());
    let result = finish_active_run(
        &mut rig, &mut owner, &mut reply, &mut trace, &mut None, stopped,
    );
    // Check before joining peers or dropping the rig: shutdown itself owns
    // reaping, including when a prior failure already occupies its result.
    for worker in [
        &mut rig.speaker,
        &mut rig.capture,
        &mut rig.provider,
        &mut rig.privacy,
    ] {
        assert!(worker.try_exit().unwrap().is_some(), "{}", worker.role);
    }
    for peer in peers {
        peer.join().unwrap();
    }
    drop(rig);
    let _ = fs::remove_dir_all(&output);
    // The microphone is closed after, whatever the run says.
    assert_eq!(
        owner.snapshot(now()).unwrap().microphone_permission(),
        Permission::Denied
    );
    assert!(reply.is_none());
    let certified: Vec<_> = trace
        .iter()
        .filter(|row| row["kind"] == "audio_diagnostics_certified")
        .map(|row| (row["worker"].clone(), row["valid"].clone()))
        .collect();
    assert_eq!(
        certified,
        [
            (json!("speaker"), json!(false)),
            (json!("capture"), json!(!capture_late)),
        ]
    );
    let forced: Vec<_> = trace
        .iter()
        .filter(|row| row["kind"] == "worker_forced_shutdown")
        .map(|row| row["worker"].clone())
        .collect();
    (result.unwrap_err().to_string(), forced)
}

/// A later child can exit while native reaping of an earlier child is in
/// progress. Its old polling result must not trigger another forced stop.
#[test]
fn capture_exiting_during_speaker_reap_is_not_forced() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    let dir = SessionDirectory::create().unwrap();
    let (mut speaker, _speaker_peer) = Worker::sleeping_fixture(&dir.path, "speaker");
    let (mut capture, _capture_peer, capture_input) =
        Worker::cooperative_ready_fixture(&dir.path, "capture");
    let (mut provider, _provider_peer, provider_input) =
        Worker::cooperative_ready_fixture(&dir.path, "provider");
    let (mut privacy, _privacy_peer, privacy_input) =
        Worker::cooperative_ready_fixture(&dir.path, "privacy");
    provider.end_on_stop(provider_input);
    privacy.end_on_stop(privacy_input);
    let capture_pid = capture.fixture_pid();
    speaker.after_terminate(move || {
        drop(capture_input);
        // Observe with WNOWAIT: only Rig::shutdown may reap this child.
        crate::scenario_clock::wait_child_exited(
            capture_pid,
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    });
    let capture_terminations = Arc::new(AtomicUsize::new(0));
    let observed = capture_terminations.clone();
    capture.after_terminate(move || {
        observed.fetch_add(1, Ordering::SeqCst);
    });
    let mut rig = Rig {
        expression_out: None,
        second: None,
        microphone: crate::options::Microphone::Mic2,
        end_silence_blocks: None,
        early_end_blocks: None,
        early_finalize: false,
        open_name: false,
        call_takes_next: false,
        unfinished_wait: false,
        faint_level: None,
        camera: None,
        motor: None,
        provider,
        capture,
        speaker,
        privacy,
        privacy_controls: false,
        capture_held: false,
        privacy_receipts: false,
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
    let (mut owner, current, _) = active_reply();
    let mut reply = Some(current);
    let mut trace = SessionState::default();
    let stopped: Result<()> = Err(io::Error::new(
        io::ErrorKind::Interrupted,
        "directed run interrupted by SIGTERM",
    )
    .into());
    let error = finish_active_run(
        &mut rig, &mut owner, &mut reply, &mut trace, &mut None, stopped,
    )
    .unwrap_err();
    assert!(error.to_string().contains("speaker worker"), "{error}");
    assert!(
        error.to_string().contains("forced termination required"),
        "{error}"
    );
    assert_eq!(capture_terminations.load(Ordering::SeqCst), 0);
    let forced: Vec<_> = trace
        .iter()
        .filter(|row| row["kind"] == "worker_forced_shutdown")
        .map(|row| row["worker"].clone())
        .collect();
    assert_eq!(forced, [json!("speaker")]);
    assert!(!trace.iter().any(|row| row["kind"] == "worker_exit_failed"));
    for worker in [
        &mut rig.speaker,
        &mut rig.capture,
        &mut rig.provider,
        &mut rig.privacy,
    ] {
        assert!(worker.try_exit().unwrap().is_some(), "{}", worker.role);
    }
    assert_eq!(rig.capture.try_exit().unwrap().unwrap().code(), Some(0));
    assert!(reply.is_none());
    assert_eq!(
        owner.snapshot(now()).unwrap().microphone_permission(),
        Permission::Denied
    );
}

fn service_signal_child(output: PathBuf, termination: Termination, mut trace: SessionState) {
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
        expression_out: None,
        second: None,
        microphone: crate::options::Microphone::Mic2,
        end_silence_blocks: None,
        early_end_blocks: None,
        early_finalize: false,
        open_name: false,
        call_takes_next: false,
        unfinished_wait: false,
        faint_level: None,
        camera: None,
        motor: None,
        provider,
        capture,
        speaker,
        privacy,
        privacy_controls: false,
        capture_held: false,
        privacy_receipts: false,
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
    // Its reply only now, its workers up (see `isolated_signal_child`).
    let (mut owner, current, _) = active_reply();
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
        Box::new(PhysicalMute {
            onboarding: None,
            observed_at_us: 1,
        }) as Box<dyn std::error::Error + Send + Sync>,
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

/// The reviewer's follow-up on #1632: the clean stop's release (#960) dates
/// the record at the conversation's last read of the press queue, not at
/// its save after the teardown, so a head hold reached during the teardown
/// is the next start's.
#[test]
fn the_clean_stops_release_dates_the_record_at_the_last_poll() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, SystemTime};
    let directory = std::env::temp_dir().join(format!("lv-clean-time-{}", monotonic_us()));
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    let record = directory.join("mute.json");
    let mut controls =
        HandControls::with_test_guard("test-hand-clock", monotonic_us(), &record).unwrap();
    controls.arm_active().unwrap();
    controls.poll().unwrap();
    // The teardown.
    std::thread::sleep(Duration::from_millis(300));
    // Against the moment just before the release, not the file's age after
    // it, so a slow save cannot pass without the time (the reviewer on #1658).
    let released = SystemTime::now();
    assert!(released_after_cleanup(&mut controls, false).unwrap());
    let before = released
        .duration_since(fs::metadata(&record).unwrap().modified().unwrap())
        .unwrap_or_default();
    let _ = fs::remove_dir_all(&directory);
    assert!(
        before >= Duration::from_millis(250),
        "{before:?} before the release: the save's time, not the last poll's"
    );
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

/// lamp-06c1, 29 Oct, 08:33:16: an update's round stopped lamp-controls
/// (16.238), the old voice's run closed on the switch it could no longer
/// read (16.37), and the voice's own stop came half a second later (16.87),
/// while it waited. The guard is released, the latch as it was, and the
/// trace says so; the lamp no longer comes back muted with nobody told.
#[test]
fn an_unreadable_switch_then_the_stop_releases_the_guard() {
    use std::os::unix::fs::PermissionsExt;
    let directory = std::env::temp_dir().join(format!("lv-unreadable-{}", monotonic_us()));
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    let record = directory.join("mute.json");
    let muted = || {
        lamp_controls::mute::MuteRecord::load(&record, "test-hand-clock", monotonic_us())
            .unwrap()
            .muted
    };
    let mut controls =
        Some(HandControls::with_test_guard("test-hand-clock", monotonic_us(), &record).unwrap());
    controls.as_mut().unwrap().arm_active().unwrap();
    controls.as_mut().unwrap().heard_audio();
    assert!(muted(), "armed while the run listened");
    // lamp-controls stops: the run closes on the switch, cleanup verified.
    let closed: Result<()> = Err(service_privacy::unavailable("lamp-controls went away"));
    note_unreadable_close(&closed, &mut controls);
    assert!(muted(), "the run's close alone keeps the guard");
    // Half a second later, while the service waits, the voice's own stop.
    std::thread::sleep(Duration::from_millis(500));
    let mut trace = SessionState::default();
    released_after_unreadable_stop(Ok(()), &mut trace, Some("SIGTERM"), &mut controls).unwrap();
    assert!(!muted(), "released: the latch as it was");
    assert!(
        trace
            .iter()
            .any(|row| row["kind"] == "guard_released_unreadable_then_stop"
                && row["signal"] == "SIGTERM")
    );
    let _ = fs::remove_dir_all(&directory);
}

/// The same close keeps the guard without the stop's signal (the wait ended
/// in a fault, or the voice exited on its own), with a press in flight or
/// taken unsaved, after a run that ended any other way, and when the close's
/// cleanup failed.
#[test]
fn an_unreadable_switch_keeps_the_guard_without_a_quiet_stop() {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::Ordering;
    let directory = std::env::temp_dir().join(format!("lv-unreadable-kept-{}", monotonic_us()));
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    let record = directory.join("mute.json");
    let muted = || {
        lamp_controls::mute::MuteRecord::load(&record, "test-hand-clock", monotonic_us())
            .unwrap()
            .muted
    };
    let unreadable =
        || -> Result<()> { Err(service_privacy::unavailable("lamp-controls went away")) };
    let closed = |result: Result<()>| {
        let mut controls = Some(
            HandControls::with_test_guard("test-hand-clock", monotonic_us(), &record).unwrap(),
        );
        controls.as_mut().unwrap().arm_active().unwrap();
        note_unreadable_close(&result, &mut controls);
        controls
    };
    let stop = |controls: &mut Option<HandControls>, result: Result<()>, signal| {
        let _ =
            released_after_unreadable_stop(result, &mut SessionState::default(), signal, controls);
    };

    let mut controls = closed(unreadable());
    stop(&mut controls, Ok(()), None);
    assert!(muted(), "no signal: not the stop's");
    stop(
        &mut controls,
        Err(io::Error::other("the wait failed").into()),
        Some("SIGTERM"),
    );
    assert!(muted(), "the wait ended in a fault");

    let mut controls = closed(unreadable());
    controls
        .as_ref()
        .unwrap()
        .queued_for_test()
        .fetch_add(1, Ordering::AcqRel);
    stop(&mut controls, Ok(()), Some("SIGTERM"));
    assert!(muted(), "a press in flight keeps the guard");

    let mut controls = closed(unreadable());
    controls.as_mut().unwrap().set_dirty_for_test();
    stop(&mut controls, Ok(()), Some("SIGTERM"));
    assert!(muted(), "a press taken and unsaved keeps the guard");

    let mut controls = closed(Err(io::Error::other("capture: a fault").into()));
    stop(&mut controls, Ok(()), Some("SIGTERM"));
    assert!(muted(), "another fault is not this rule's");

    let mut controls = closed(service_privacy::finish(
        unreadable(),
        Err(io::Error::other("endpoint worker exit not confirmed")),
    ));
    stop(&mut controls, Ok(()), Some("SIGTERM"));
    assert!(muted(), "a failed cleanup hides the switch's error");

    // A later run that ends another way clears the close.
    let mut controls = closed(unreadable());
    note_unreadable_close(&Err(io::Error::other("a fault").into()), &mut controls);
    stop(&mut controls, Ok(()), Some("SIGTERM"));
    assert!(muted(), "the last run's close, not an earlier one's");
    let _ = fs::remove_dir_all(&directory);
}
