//! Scheduler checks exchange real control frames with mock participants. Their
//! receipts are not recording, runtime, signal or process-exit evidence. The
//! backend's separate tests retain the actual owned-Child exit barrier.
use super::*;
use crate::{
    physical::{fixture_control::Participant, session::RuntimeFlags},
    record::StimulusSource,
};
use std::{
    io::{self, Read},
    os::unix::{fs::DirBuilderExt, net::UnixStream},
    sync::{Arc, Mutex, mpsc},
    thread::JoinHandle,
    time::Duration,
};

const WATCHDOG: Duration = Duration::from_secs(30);
type Observations = Arc<Mutex<Vec<(Role, Step)>>>;

struct Private(PathBuf);

impl Private {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = PathBuf::from(format!(
            "/tmp/lve-recorder-driver-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
}

impl Drop for Private {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn config() -> PhysicalConfig {
    PhysicalConfig {
        lamp_command: [
            "/unused/lamp-voice-eval",
            "lamp-session",
            "--runtime",
            "/unused/lamp-voice-eval",
            "--runtime-prefix-arg",
            "fake-lamp-voice",
            "--runtime-prefix-arg",
            "--fake-room",
            "--runtime-prefix-arg",
            "/private/room with spaces.sock",
        ]
        .map(str::to_owned)
        .into(),
        lamp_work_root: "/unused/work".into(),
        mode: LampMode::Serve {
            provider_config: "/unused/provider.json".into(),
        },
        noise_suppression: "on".into(),
        diagnostics: false,
        room_recorder: Some(
            [
                "/unused/lamp-voice-eval",
                "fake-room-recorder",
                "--out",
                "{out}",
                "--seconds",
                "{seconds}",
                "--fail-after-ms",
                "6000",
                "--exit-code",
                "7",
                "--declare-end-after-ms",
                "5000",
            ]
            .map(str::to_owned)
            .into(),
        ),
        room_independent: false,
        allowance_seconds: 10,
        ring_channel_ceiling: None,
        runtime_flags: RuntimeFlags::default(),
        stimulus_source: StimulusSource::LoudspeakerSynthetic,
    }
}

fn configured(directory: &Private) -> ClockedServeFixture {
    let mut fixture = ClockedServeFixture::new(&directory.0).unwrap();
    fixture.configure(&mut config()).unwrap();
    assert!(fixture.supports_serve());
    assert!(fixture.supports_recorder());
    fixture
}

fn steps(observations: &Observations, role: Role) -> Vec<Step> {
    observations
        .lock()
        .unwrap()
        .iter()
        .filter_map(|(observed_role, step)| (*observed_role == role).then_some(*step))
        .collect()
}

fn times(observations: &Observations, role: Role) -> Vec<u64> {
    steps(observations, role)
        .into_iter()
        .map(|step| step.at_us)
        .collect()
}

fn observe(participant: &Participant, role: Role, observations: &Observations) -> Step {
    let step = participant.current_step();
    assert_eq!(participant.clock().now().as_micros(), step.at_us);
    assert_eq!(participant.clock().now().as_micros(), step.at_us);
    assert!(!step.wait_for_sigterm);
    observations.lock().unwrap().push((role, step));
    step
}

fn next_or_closed(participant: &mut Participant, progress: Progress) -> bool {
    match participant.complete(progress) {
        Ok(_) => true,
        // The parent closes only after consuming this Ack. A continuing mock
        // is parked in its next-Step read when this explicit cleanup occurs.
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => false,
        Err(error) => panic!("unexpected mock participant failure: {error}"),
    }
}

fn spawn_recorder(
    fixture: &ClockedServeFixture,
    exit_due_us: u64,
    observations: &Observations,
) -> JoinHandle<()> {
    let path = fixture.control_path.clone();
    let observations = Arc::clone(observations);
    thread::spawn(move || {
        let mut participant =
            Participant::connect_with_watchdog(&path, Role::Recorder, WATCHDOG).unwrap();
        let wait_started_us = participant.current_step().at_us;
        loop {
            let step = observe(&participant, Role::Recorder, &observations);
            assert_eq!(step.stdout_target, 0);
            assert_eq!(step.runner_input_target, 0);
            let progress = Progress::Recorder {
                poll_seq: step.round,
                ready_written: true,
                wait_started_us,
                exit_due_us,
                logical_end: step.at_us >= exit_due_us,
            };
            if progress.logical_end() {
                participant.final_ack(progress).unwrap();
                return;
            }
            if !next_or_closed(&mut participant, progress) {
                return;
            }
        }
    })
}

fn spawn_serve(
    fixture: &ClockedServeFixture,
    relay_terminal_round: Option<u64>,
    observations: &Observations,
) -> [JoinHandle<()>; 2] {
    let relay_path = fixture.control_path.clone();
    let runtime_path = fixture.control_path.clone();
    let relay_observations = Arc::clone(observations);
    let runtime_observations = Arc::clone(observations);
    let (relay_started, start_runtime) = mpsc::channel();
    let relay = thread::spawn(move || {
        let mut participant =
            Participant::connect_with_watchdog(&relay_path, Role::ServeRelay, WATCHDOG).unwrap();
        // The runtime connects only after the parent has actually accepted
        // the relay. Host scheduling cannot reorder the registration roles.
        relay_started.send(()).unwrap();
        loop {
            let step = observe(&participant, Role::ServeRelay, &relay_observations);
            let progress = Progress::ServeRelay {
                poll_seq: step.round,
                stdout_frames: step.stdout_target,
                runner_inputs: step.runner_input_target,
                output_frames: 0,
                stop_signal_sent_us: None,
                logical_end: relay_terminal_round == Some(step.round),
            };
            if progress.logical_end() {
                participant.final_ack(progress).unwrap();
                return;
            }
            if !next_or_closed(&mut participant, progress) {
                return;
            }
        }
    });
    let runtime = thread::spawn(move || {
        start_runtime.recv_timeout(WATCHDOG).unwrap();
        let mut participant =
            Participant::connect_with_watchdog(&runtime_path, Role::ServeRuntime, WATCHDOG)
                .unwrap();
        loop {
            let step = observe(&participant, Role::ServeRuntime, &runtime_observations);
            let progress = Progress::ServeRuntime {
                poll_seq: step.round,
                stdout_frames: 0,
                room_consumed: 0,
                room_injected: 0,
                cycle: 1,
                cycle_base_us: BOOTSTRAP_US,
                runtime_us: step.at_us - BOOTSTRAP_US,
                observed_signal: 0,
                logical_end: false,
            };
            if !next_or_closed(&mut participant, progress) {
                return;
            }
        }
    });
    [relay, runtime]
}

fn advance(fixture: &mut ClockedServeFixture, until: u64) {
    fixture
        .wait(MonoTime::from_micros(until), 0, &AtomicU64::new(0), true)
        .unwrap();
}

fn close_and_join(
    fixture: &mut ClockedServeFixture,
    workers: impl IntoIterator<Item = JoinHandle<()>>,
) {
    fixture.close();
    for worker in workers {
        worker.join().unwrap();
    }
}

#[test]
fn recorder_starts_at_the_held_epoch_and_clock_reads_do_not_poll_it() {
    let directory = Private::new();
    let mut fixture = configured(&directory);
    let observations = Observations::default();
    let worker = spawn_recorder(&fixture, EPOCH_US + 6_000, &observations);
    fixture.start_recorder(Instant::now() + WATCHDOG).unwrap();
    assert_eq!(fixture.clock().now().as_micros(), EPOCH_US);
    assert_eq!(fixture.clock().now().as_micros(), EPOCH_US);
    advance(&mut fixture, EPOCH_US);
    assert_eq!(times(&observations, Role::Recorder), [EPOCH_US]);
    let recorder = fixture.recorder.as_ref().unwrap();
    assert_eq!(recorder.round, 1);
    assert_eq!(recorder.wait_started_us, Some(EPOCH_US));
    assert_eq!(recorder.exit_due_us, Some(EPOCH_US + 6_000));
    assert!(!fixture.recorder_exit_pending());
    assert!(!fixture.started);
    close_and_join(&mut fixture, [worker]);
}

#[test]
fn recorder_due_before_or_on_a_poll_preserves_the_pending_runtime_deadline() {
    for exit_due_us in [BOOTSTRAP_US + POLL_US - 1, BOOTSTRAP_US + POLL_US] {
        let directory = Private::new();
        let mut fixture = configured(&directory);
        let observations = Observations::default();
        let recorder = spawn_recorder(&fixture, exit_due_us, &observations);
        fixture.start_recorder(Instant::now() + WATCHDOG).unwrap();
        let serve = spawn_serve(&fixture, None, &observations);
        let later = BOOTSTRAP_US + 2 * POLL_US;
        advance(&mut fixture, later);
        assert_eq!(times(&observations, Role::ServeRuntime), [BOOTSTRAP_US]);
        advance(&mut fixture, later);
        assert_eq!(fixture.clock().now().as_micros(), exit_due_us);
        assert!(fixture.recorder_exit_pending());
        assert!(fixture.require_recorder_exit_observed().is_err());
        assert_eq!(fixture.next_poll_us, BOOTSTRAP_US + POLL_US);
        assert_eq!(times(&observations, Role::ServeRuntime), [BOOTSTRAP_US]);
        assert_eq!(
            times(&observations, Role::ServeRelay),
            [BOOTSTRAP_US, BOOTSTRAP_US]
        );
        // This notification models only the backend's explicit API call.
        // No mock thread completion is interpreted as a real Child exit.
        fixture.recorder_exit_observed().unwrap();
        assert_eq!(fixture.clock().now().as_micros(), exit_due_us);
        advance(&mut fixture, later);
        assert_eq!(
            times(&observations, Role::ServeRuntime),
            [BOOTSTRAP_US, BOOTSTRAP_US + POLL_US]
        );
        assert_eq!(
            times(&observations, Role::ServeRelay),
            [BOOTSTRAP_US, BOOTSTRAP_US, BOOTSTRAP_US + POLL_US]
        );
        assert_eq!(
            times(&observations, Role::Recorder),
            [EPOCH_US, BOOTSTRAP_US, exit_due_us]
        );
        assert_eq!(fixture.next_poll_us, later);
        assert!(!fixture.recorder_exit_pending());
        assert!(fixture.recorder_exit_observed().is_err());
        close_and_join(&mut fixture, [recorder].into_iter().chain(serve));
    }
}

#[test]
fn recorder_due_before_or_on_bootstrap_holds_registration_until_exit_notification() {
    for exit_due_us in [BOOTSTRAP_US - 1, BOOTSTRAP_US] {
        let directory = Private::new();
        let mut fixture = configured(&directory);
        let observations = Observations::default();
        let recorder = spawn_recorder(&fixture, exit_due_us, &observations);
        fixture.start_recorder(Instant::now() + WATCHDOG).unwrap();
        let later = BOOTSTRAP_US + POLL_US;
        advance(&mut fixture, later);
        assert_eq!(fixture.clock().now().as_micros(), exit_due_us);
        assert!(fixture.recorder_exit_pending());
        assert!(!fixture.started);
        assert!(fixture.relay.is_none());
        assert!(fixture.runtime.is_none());
        fixture.recorder_exit_observed().unwrap();
        let serve = spawn_serve(&fixture, None, &observations);
        advance(&mut fixture, later);
        assert!(fixture.started);
        assert_eq!(fixture.clock().now().as_micros(), BOOTSTRAP_US);
        assert_eq!(
            times(&observations, Role::Recorder),
            [EPOCH_US, exit_due_us]
        );
        assert_eq!(times(&observations, Role::ServeRuntime), [BOOTSTRAP_US]);
        close_and_join(&mut fixture, [recorder].into_iter().chain(serve));
    }
}

#[test]
fn terminal_recorder_ack_cannot_authorize_a_clock_advance() {
    let directory = Private::new();
    let mut fixture = configured(&directory);
    let observations = Observations::default();
    let recorder = spawn_recorder(&fixture, EPOCH_US, &observations);
    fixture.start_recorder(Instant::now() + WATCHDOG).unwrap();
    assert!(fixture.recorder_exit_pending());
    let error = fixture
        .wait(
            MonoTime::from_micros(BOOTSTRAP_US),
            0,
            &AtomicU64::new(0),
            true,
        )
        .unwrap_err();
    assert!(error.to_string().contains("actual child exit"));
    assert_eq!(fixture.clock().now().as_micros(), EPOCH_US);
    assert_eq!(times(&observations, Role::Recorder), [EPOCH_US]);
    assert!(fixture.failed);
    assert!(fixture.listener.is_none());
    assert!(fixture.recorder.is_none());
    recorder.join().unwrap();
}

#[test]
fn recorder_drains_after_relay_completion_without_stepping_the_runtime_again() {
    let directory = Private::new();
    let mut fixture = configured(&directory);
    let observations = Observations::default();
    let exit_due_us = BOOTSTRAP_US + 3 * POLL_US + 1;
    let recorder = spawn_recorder(&fixture, exit_due_us, &observations);
    fixture.start_recorder(Instant::now() + WATCHDOG).unwrap();
    let serve = spawn_serve(&fixture, Some(3), &observations);
    advance(&mut fixture, exit_due_us + POLL_US);
    advance(&mut fixture, exit_due_us + POLL_US);
    assert!(fixture.relay_done);
    assert!(fixture.runtime.is_none());
    assert_eq!(fixture.clock().now().as_micros(), BOOTSTRAP_US + POLL_US);
    let runtime_before = steps(&observations, Role::ServeRuntime);
    let relay_before = steps(&observations, Role::ServeRelay);
    fixture.finish_recorder(Instant::now() + WATCHDOG).unwrap();
    assert_eq!(fixture.clock().now().as_micros(), exit_due_us);
    assert!(fixture.recorder_exit_pending());
    assert_eq!(steps(&observations, Role::ServeRuntime), runtime_before);
    assert_eq!(steps(&observations, Role::ServeRelay), relay_before);
    assert_eq!(
        times(&observations, Role::Recorder),
        [
            EPOCH_US,
            BOOTSTRAP_US,
            BOOTSTRAP_US + POLL_US,
            BOOTSTRAP_US + 2 * POLL_US,
            BOOTSTRAP_US + 3 * POLL_US,
            exit_due_us,
        ]
    );
    fixture.recorder_exit_observed().unwrap();
    fixture.finish_players(Instant::now() + WATCHDOG).unwrap();
    assert_eq!(fixture.clock().now().as_micros(), exit_due_us);
    close_and_join(&mut fixture, [recorder].into_iter().chain(serve));
}

#[test]
fn players_cannot_finish_before_the_recorder_exits_even_without_pending_players() {
    for duration_us in [0, 6_000] {
        let directory = Private::new();
        let mut fixture = configured(&directory);
        let observations = Observations::default();
        let recorder = spawn_recorder(&fixture, EPOCH_US + duration_us, &observations);
        fixture.start_recorder(Instant::now() + WATCHDOG).unwrap();
        assert!(fixture.players.all_reports_ready());
        let error = fixture
            .finish_players(Instant::now() + WATCHDOG)
            .unwrap_err();
        assert!(error.to_string().contains("recorder"));
        assert_eq!(fixture.clock().now().as_micros(), EPOCH_US);
        assert!(fixture.failed);
        assert!(fixture.recorder.is_none());
        assert!(fixture.listener.is_none());
        recorder.join().unwrap();
    }
}

#[test]
fn recorder_configuration_preserves_every_argument_and_placeholder() {
    let directory = Private::new();
    let mut fixture = ClockedServeFixture::new(&directory.0).unwrap();
    let mut config = config();
    let original = config.room_recorder.clone().unwrap();
    fixture.configure(&mut config).unwrap();
    let changed = config.room_recorder.as_ref().unwrap();
    assert_eq!(
        &changed[..4],
        [
            original[0].as_str(),
            "fake-clock-room-recorder",
            "--fixture-control",
            fixture.control_path.to_str().unwrap(),
        ]
    );
    assert_eq!(&changed[4..], &original[2..]);
    assert!(changed.iter().any(|value| value == "{out}"));
    assert!(changed.iter().any(|value| value == "{seconds}"));
    assert!(fixture.supports_recorder());
    assert!(fixture.configure(&mut config).is_err());
}

#[test]
fn wrapper_other_runtime_and_invalid_recorder_options_fail_without_mutating_config() {
    let directory = Private::new();
    let mut fixture = ClockedServeFixture::new(&directory.0).unwrap();
    let original = config().room_recorder.unwrap();
    let mut invalid = vec![Vec::new(), vec![original[0].clone()]];
    for (index, value) in [
        (0, "/bin/sh"),
        (0, "/other/lamp-voice-eval"),
        (1, "fake-slow-transport"),
        (1, "fake-lamp-voice"),
        (2, "--unrecognized"),
    ] {
        let mut command = original.clone();
        command[index] = value.into();
        invalid.push(command);
    }
    let mut incomplete = original;
    incomplete.pop();
    invalid.push(incomplete);
    for recorder in invalid {
        let mut config = config();
        config.room_recorder = Some(recorder.clone());
        let lamp_command = config.lamp_command.clone();
        assert!(fixture.configure(&mut config).is_err(), "{recorder:?}");
        assert_eq!(config.lamp_command, lamp_command);
        assert_eq!(config.room_recorder, Some(recorder));
        assert!(!fixture.configured);
        assert!(!fixture.supports_recorder());
    }
    let mut wrong_runtime = config();
    wrong_runtime.lamp_command[3] = "/other/lamp-voice-eval".into();
    let lamp_command = wrong_runtime.lamp_command.clone();
    let recorder = wrong_runtime.room_recorder.clone();
    assert!(fixture.configure(&mut wrong_runtime).is_err());
    assert_eq!(wrong_runtime.lamp_command, lamp_command);
    assert_eq!(wrong_runtime.room_recorder, recorder);
    assert!(!fixture.configured);
}

#[test]
fn expired_recorder_startup_watchdog_closes_the_listener_without_advancing_time() {
    let directory = Private::new();
    let mut fixture = configured(&directory);
    // Queue a real connection before expiring the accept budget. No assertion
    // requires Hello or Step delivery after that deadline has already passed.
    let mut peer = UnixStream::connect(&fixture.control_path).unwrap();
    // The read below waits for the close, as long as the fixture's watchdog:
    // a process another test spawns at this moment holds the queued
    // connection until it exits. Beside a thread spawning `sleep 0.3` in a
    // loop, a read straight after the close found it still open in 151 and
    // 157 of 300 rounds (none beside CPU load alone). This read found it
    // closed in all 300, after at most 310 ms (#1569 row 28). The timeout
    // is set now: macOS refuses it (EINVAL) once the peer has closed.
    peer.set_read_timeout(Some(WATCHDOG)).unwrap();
    let expired = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
    let error = fixture.start_recorder(expired).unwrap_err();
    assert_eq!(
        error.downcast_ref::<io::Error>().unwrap().kind(),
        io::ErrorKind::TimedOut
    );
    assert_eq!(fixture.clock().now().as_micros(), EPOCH_US);
    assert!(fixture.failed);
    assert!(fixture.listener.is_none());
    assert!(fixture.recorder.is_none());
    assert!(!fixture.control_path.exists());
    match peer.read(&mut [0; 1]) {
        Ok(0) => {}
        Err(error) if error.kind() == io::ErrorKind::ConnectionReset => {}
        result => panic!("expired startup kept its queued connection open: {result:?}"),
    }
    assert!(fixture.start_recorder(Instant::now() + WATCHDOG).is_err());
    assert_eq!(fixture.clock().now().as_micros(), EPOCH_US);
}
