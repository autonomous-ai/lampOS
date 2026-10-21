//! Parent scheduler for one explicit, finite local physical-loopback attempt.
//! The real runtime, relay, sockets, framing and backend remain in the path.
//! Clock reads are pure; only a backend wait advances the fixture schedule.
use super::{
    fixture_control::{DEFAULT_WATCHDOG, ParentListener, ParentPeer, Progress, Role, Step},
    mac::{
        FIXTURE_RELAY_QUEUE_CAPACITY, FakeRoomPlayer, FixtureDriver, LampMode, PhysicalConfig,
        Player,
    },
};
use crate::{Result, invalid, stimulus::SceneTiming};
use lamp_turn_owner::MonoTime;
use serde_json::Value;
use std::{
    cell::Cell,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const POLL_US: u64 = 2_000;
/// The poll while a round has changed nothing: the fake runtime's own tick,
/// as the serve fixture does. Most of a physical loopback's rounds change
/// nothing, and at 2 ms throughout its tests took 1.0 to 1.6 s (#1806).
/// After any round that changes something, the next is the lamp's 2 ms again.
const IDLE_POLL_US: u64 = crate::fake::TICK_US;
const BOOTSTRAP_US: u64 = POLL_US;

/// A read-only handle for backend and player timestamps. It owns no control
/// socket and cannot release a participant or advance the fixture.
#[derive(Clone, Debug)]
pub struct RunnerFixtureClock(Arc<AtomicU64>);

impl RunnerFixtureClock {
    pub(crate) fn new(at_us: u64) -> Self {
        Self(Arc::new(AtomicU64::new(at_us)))
    }

    pub fn now(&self) -> MonoTime {
        MonoTime::from_micros(self.0.load(Ordering::Acquire))
    }

    pub(crate) fn advance(&self, at_us: u64) -> Result<()> {
        if at_us < self.now().as_micros() {
            return Err(invalid("fixture driver clock would regress"));
        }
        self.0.store(at_us, Ordering::Release);
        Ok(())
    }
}

/// Coordinates one finite fake runtime and its real relay. Construct this in
/// the attempt's private directory before spawning either child, configure the
/// local command, then move it into `PhysicalBackend::with_fixture_driver`.
pub struct ClockedFixture {
    relay: Option<ParentPeer>,
    runtime: Option<ParentPeer>,
    listener: Option<ParentListener>,
    control_path: PathBuf,
    clock: RunnerFixtureClock,
    expected_room_inputs: Rc<Cell<u64>>,
    configured: bool,
    started: bool,
    failed: bool,
    runtime_round: u64,
    relay_round: u64,
    runtime_stdout: u64,
    runtime_rooms: u64,
    runner_inputs: u64,
    output_frames: u64,
    drained_frames: u64,
    next_poll_us: u64,
    runtime_done: bool,
    relay_done: bool,
    terminal_until: Option<Instant>,
}

impl ClockedFixture {
    pub fn new(private_root: &Path) -> Result<Self> {
        let metadata = fs::symlink_metadata(private_root)?;
        if !private_root.is_absolute()
            || !metadata.is_dir()
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(invalid(
                "fixture control needs an absolute caller-owned private directory",
            ));
        }
        let control_path = private_root.join("fixture.sock");
        let listener = ParentListener::bind(&control_path)?;
        Ok(Self {
            relay: None,
            runtime: None,
            listener: Some(listener),
            control_path,
            clock: RunnerFixtureClock(Arc::new(AtomicU64::new(0))),
            expected_room_inputs: Rc::new(Cell::new(0)),
            configured: false,
            started: false,
            failed: false,
            runtime_round: 0,
            relay_round: 0,
            runtime_stdout: 0,
            runtime_rooms: 0,
            runner_inputs: 0,
            output_frames: 0,
            drained_frames: 0,
            next_poll_us: BOOTSTRAP_US,
            runtime_done: false,
            relay_done: false,
            terminal_until: None,
        })
    }

    pub fn clock(&self) -> RunnerFixtureClock {
        self.clock.clone()
    }

    /// Select only the explicitly named local fake commands. Existing fake
    /// scenario, seed, duration and fault arguments keep their order and values.
    /// No ambient environment variable enables the controlled path.
    pub fn configure(&mut self, config: &mut PhysicalConfig) -> Result<()> {
        if self.configured || self.started {
            return Err(invalid(
                "fixture driver can configure only one fresh attempt",
            ));
        }
        if matches!(config.mode, LampMode::Serve { .. }) || config.room_recorder.is_some() {
            return Err(invalid(
                "fixture stepping supports finite sessions without a recorder",
            ));
        }
        let command = &config.lamp_command;
        let executable = command
            .first()
            .ok_or_else(|| invalid("missing local evaluator command"))?;
        let executable_path = Path::new(executable);
        if !executable_path.is_absolute()
            || executable_path.file_name().and_then(|name| name.to_str()) != Some("lamp-voice-eval")
            || command.get(1).map(String::as_str) != Some("lamp-session")
        {
            return Err(invalid(
                "fixture stepping requires a direct local lamp-voice-eval lamp-session command",
            ));
        }
        let mut runtime = None;
        let mut prefix = Vec::new();
        let mut index = 2;
        while index < command.len() {
            let value = command
                .get(index + 1)
                .ok_or_else(|| invalid("incomplete fixture command option"))?;
            match command[index].as_str() {
                "--runtime" if runtime.is_none() => runtime = Some(value),
                "--runtime-prefix-arg" => prefix.push(value.clone()),
                _ => {
                    return Err(invalid(
                        "fixture command must contain only one runtime and its explicit prefix arguments",
                    ));
                }
            }
            index += 2;
        }
        if runtime != Some(executable)
            || prefix.first().map(String::as_str) != Some("fake-lamp-voice")
        {
            return Err(invalid(
                "fixture stepping requires the same local evaluator's finite fake runtime",
            ));
        }
        if prefix.len() % 2 != 1
            || prefix[1..]
                .chunks_exact(2)
                .any(|pair| !pair[0].starts_with("--fake-"))
        {
            return Err(invalid(
                "fixture runtime prefix must contain explicit --fake-* option/value pairs",
            ));
        }
        let control = self
            .control_path
            .to_str()
            .ok_or_else(|| invalid("fixture control path must be UTF-8"))?;
        let mut configured = vec![
            executable.clone(),
            "fake-clock-session".into(),
            "--fixture-control".into(),
            control.into(),
            "--runtime".into(),
            executable.clone(),
        ];
        for value in ["fake-clock-lamp-voice", "--fixture-control", control] {
            configured.extend(["--runtime-prefix-arg".into(), value.into()]);
        }
        for value in &prefix[1..] {
            configured.extend(["--runtime-prefix-arg".into(), value.clone()]);
        }
        config.lamp_command = configured;
        self.configured = true;
        Ok(())
    }

    /// Wrap only the immediate fake room player. A successful call means its
    /// datagram was sent; the next runtime poll must separately acknowledge
    /// consuming it. Failed sends do not increase the expected input count.
    pub fn player(&self, room: PathBuf) -> FixtureRoomPlayer {
        let clock = self.clock();
        FixtureRoomPlayer {
            inner: Box::new(FakeRoomPlayer { room }.with_clock(move || clock.now())),
            expected: Rc::clone(&self.expected_room_inputs),
        }
    }

    fn close_peers(&mut self) {
        drop(self.runtime.take());
        drop(self.relay.take());
        drop(self.listener.take());
    }

    fn runtime_step(&mut self, until: Instant) -> Result<()> {
        let round = next_round(self.runtime_round)?;
        let progress = self
            .runtime
            .as_mut()
            .ok_or_else(|| invalid("fixture runtime is not connected"))?
            .step_until(
                Step {
                    round,
                    at_us: self.clock.now().as_micros(),
                    stdout_target: 0,
                    runner_input_target: 0,
                    wait_for_sigterm: false,
                },
                until,
            )?;
        let Progress::Runtime {
            stdout_frames,
            room_inputs,
            logical_end,
            ..
        } = progress
        else {
            return Err(invalid(
                "fixture runtime returned another participant's progress",
            ));
        };
        if room_inputs != self.expected_room_inputs.get() {
            return Err(invalid(&format!(
                "fixture runtime consumed {room_inputs} room inputs; {} successful player sends were expected",
                self.expected_room_inputs.get()
            )));
        }
        self.runtime_round = round;
        self.runtime_stdout = stdout_frames;
        self.runtime_rooms = room_inputs;
        self.runtime_done = logical_end;
        if logical_end {
            self.terminal_until = Some(Instant::now() + DEFAULT_WATCHDOG);
        }
        Ok(())
    }

    fn relay_step(&mut self, sent_frames: u64, enqueued: &AtomicU64, until: Instant) -> Result<()> {
        let round = next_round(self.relay_round)?;
        let progress = self
            .relay
            .as_mut()
            .ok_or_else(|| invalid("fixture relay is not connected"))?
            .step_until(
                Step {
                    round,
                    at_us: self.clock.now().as_micros(),
                    stdout_target: self.runtime_stdout,
                    runner_input_target: sent_frames,
                    wait_for_sigterm: false,
                },
                until,
            )?;
        let Progress::Relay {
            stdout_frames,
            runner_inputs,
            output_frames,
            logical_end,
            ..
        } = progress
        else {
            return Err(invalid(
                "fixture relay returned another participant's progress",
            ));
        };
        if stdout_frames != self.runtime_stdout || runner_inputs != sent_frames {
            return Err(invalid(&format!(
                "fixture relay consumed stdout/input frames {stdout_frames}/{runner_inputs}; parked producers announced {}/{sent_frames}",
                self.runtime_stdout
            )));
        }
        if output_frames < self.output_frames {
            return Err(invalid("fixture relay output count regressed"));
        }
        ensure_queue_capacity(output_frames, self.drained_frames)?;
        wait_for_enqueued(enqueued, output_frames, until)?;
        self.relay_round = round;
        self.runner_inputs = runner_inputs;
        self.output_frames = output_frames;
        self.relay_done = logical_end;
        Ok(())
    }

    fn finish_runtime_exit(&mut self, sent_frames: u64, enqueued: &AtomicU64) -> Result<()> {
        if !self.runtime_done || self.relay_done {
            return Ok(());
        }
        let until = self
            .terminal_until
            .ok_or_else(|| invalid("missing fixture terminal deadline"))?;
        // Logical end is not process exit. Repeated rounds at the same time
        // allow the relay to observe try_wait, drain stdout, and flush the final
        // trace/SessionEnd. One host deadline bounds the entire sequence.
        while !self.relay_done {
            check_host_deadline(until, "runtime exit and final relay output")?;
            self.relay_step(sent_frames, enqueued, until)?;
            thread::yield_now();
        }
        Ok(())
    }

    fn bootstrap(&mut self, sent_frames: u64, enqueued: &AtomicU64) -> Result<()> {
        self.clock.advance(BOOTSTRAP_US)?;
        let listener = self
            .listener
            .as_ref()
            .ok_or_else(|| invalid("fixture listener is closed"))?;
        self.relay = Some(listener.accept(Role::Relay)?);
        // The initial Ping was sent at zero. SessionStart at 2 ms deliberately
        // cannot certify that setup Ping as a live clock-mapping sample.
        self.relay_step(sent_frames, enqueued, Instant::now() + DEFAULT_WATCHDOG)?;
        if self.relay_done {
            return Err(invalid("fixture relay ended before runtime registration"));
        }
        self.runtime = Some(
            self.listener
                .as_ref()
                .ok_or_else(|| invalid("fixture listener is closed"))?
                .accept(Role::Runtime)?,
        );
        let until = Instant::now() + DEFAULT_WATCHDOG;
        self.runtime_step(until)?;
        self.relay_step(sent_frames, enqueued, until)?;
        self.started = true;
        self.next_poll_us = BOOTSTRAP_US + POLL_US;
        self.finish_runtime_exit(sent_frames, enqueued)
    }

    fn wait_inner(
        &mut self,
        until: MonoTime,
        sent_frames: u64,
        enqueued: &AtomicU64,
        queue_drained: bool,
    ) -> Result<()> {
        if !self.configured || self.failed {
            return Err(invalid(
                "fixture driver is unconfigured or closed after a failure",
            ));
        }
        if sent_frames < self.runner_inputs || self.expected_room_inputs.get() < self.runtime_rooms
        {
            return Err(invalid("fixture producer counters regressed"));
        }
        if queue_drained {
            self.drained_frames = enqueued.load(Ordering::Acquire);
        }
        let at_us = self.clock.now().as_micros();
        let until_us = until.as_micros();
        if until_us <= at_us {
            return Ok(());
        }
        if !self.started {
            if until_us < BOOTSTRAP_US {
                return self.clock.advance(until_us);
            }
            return self.bootstrap(sent_frames, enqueued);
        }
        if self.relay_done {
            // No participant work remains. next_event calls only with a
            // drained queue; inject may retain it until its original final
            // dispatch drain, just as it did while sleeping on the host.
            return self.clock.advance(until_us);
        }
        if self.runtime_done {
            return self.finish_runtime_exit(sent_frames, enqueued);
        }
        let next = until_us.min(self.next_poll_us);
        self.clock.advance(next)?;
        if next < self.next_poll_us {
            return Ok(());
        }
        let host_until = Instant::now() + DEFAULT_WATCHDOG;
        let before = self.progress();
        self.runtime_step(host_until)?;
        self.relay_step(sent_frames, enqueued, host_until)?;
        let step = if self.progress() == before {
            IDLE_POLL_US
        } else {
            POLL_US
        };
        self.next_poll_us = next
            .checked_add(step)
            .ok_or_else(|| invalid("fixture poll time overflow"))?;
        self.finish_runtime_exit(sent_frames, enqueued)
    }

    /// What a round can change: anything said, room, input or frame.
    fn progress(&self) -> (u64, u64, u64, u64, u64) {
        (
            self.runtime_stdout,
            self.runtime_rooms,
            self.runner_inputs,
            self.output_frames,
            self.expected_room_inputs.get(),
        )
    }
}

impl FixtureDriver for ClockedFixture {
    fn wait(
        &mut self,
        until: MonoTime,
        sent_frames: u64,
        enqueued_frames: &AtomicU64,
        queue_drained: bool,
    ) -> Result<()> {
        let result = self.wait_inner(until, sent_frames, enqueued_frames, queue_drained);
        if result.is_err() {
            self.failed = true;
            self.close_peers();
        }
        result
    }
}

impl Drop for ClockedFixture {
    fn drop(&mut self) {
        self.close_peers();
    }
}

pub struct FixtureRoomPlayer {
    inner: Box<dyn Player>,
    expected: Rc<Cell<u64>>,
}

impl Player for FixtureRoomPlayer {
    fn start(
        &mut self,
        wav: &Path,
        timing: &SceneTiming,
        report_directory: &Path,
        cancel_file: &Path,
    ) -> Result<JoinHandle<Value>> {
        let next = self
            .expected
            .get()
            .checked_add(1)
            .ok_or_else(|| invalid("fixture room input counter overflow"))?;
        let playback = self
            .inner
            .start(wav, timing, report_directory, cancel_file)?;
        self.expected.set(next);
        Ok(playback)
    }

    fn describe(&self) -> Value {
        self.inner.describe()
    }
}

pub(crate) fn next_round(previous: u64) -> Result<u64> {
    previous
        .checked_add(1)
        .ok_or_else(|| invalid("fixture round overflow"))
}

/// Share the backend's finite-fake transcript bound: all trace and cue frames,
/// bounded pongs, readiness/start/end, and the explicit duplicate-cue fault fit
/// before a terminal acknowledgement. The backend reserves its extra error slot
/// separately; it is never counted as room for normal announced output.
pub(crate) fn ensure_queue_capacity(announced: u64, drained: u64) -> Result<()> {
    if announced < drained {
        return Err(invalid(
            "fixture output announcement is behind the drained frame count",
        ));
    }
    if announced.saturating_sub(drained) > FIXTURE_RELAY_QUEUE_CAPACITY as u64 {
        return Err(invalid(&format!(
            "fixture announced {announced} output frames after {drained} drained frames, exceeding the {FIXTURE_RELAY_QUEUE_CAPACITY}-frame finite-fixture backend queue"
        )));
    }
    Ok(())
}

pub(crate) fn check_host_deadline(until: Instant, what: &str) -> Result<Duration> {
    until
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| invalid(&format!("fixture {what} exceeded its host watchdog")))
}

pub(crate) fn wait_for_enqueued(enqueued: &AtomicU64, target: u64, until: Instant) -> Result<()> {
    while enqueued.load(Ordering::Acquire) < target {
        let remaining = check_host_deadline(until, "backend reader enqueue barrier")?;
        thread::sleep(remaining.min(Duration::from_millis(1)));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        physical::{fixture_control::Participant, session::RuntimeFlags},
        record::StimulusSource,
        stimulus::TimingSource,
    };
    use std::os::unix::{fs::DirBuilderExt, net::UnixDatagram};

    struct Private(PathBuf);
    impl Private {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = PathBuf::from(format!(
                "/tmp/lve-driver-{}-{}",
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
                "--fake-scenario",
                "--runtime-prefix-arg",
                "fixed-reply-echo-only",
                "--runtime-prefix-arg",
                "--fake-room",
                "--runtime-prefix-arg",
                "/private/room with spaces.sock",
                "--runtime-prefix-arg",
                "--fake-cue-fault",
                "--runtime-prefix-arg",
                "drop-completion",
            ]
            .map(str::to_owned)
            .into(),
            lamp_work_root: "/unused/work".into(),
            mode: LampMode::Fixture {
                reply: "/unused/reply.wav".into(),
                sha256: "0".repeat(64),
            },
            noise_suppression: "on".into(),
            diagnostics: false,
            room_recorder: None,
            room_independent: false,
            allowance_seconds: 10,
            ring_channel_ceiling: None,
            runtime_flags: RuntimeFlags::default(),
            stimulus_source: StimulusSource::LoudspeakerSynthetic,
        }
    }

    #[test]
    fn configuration_changes_only_explicit_fake_launchers_and_control_arguments() {
        let directory = Private::new();
        let mut fixture = ClockedFixture::new(&directory.0).unwrap();
        let mut config = config();
        let original = config.lamp_command.clone();
        fixture.configure(&mut config).unwrap();
        let control = fixture.control_path.to_str().unwrap();
        assert_eq!(
            &config.lamp_command[..6],
            [
                original[0].as_str(),
                "fake-clock-session",
                "--fixture-control",
                control,
                "--runtime",
                original[0].as_str()
            ]
        );
        let prefix: Vec<_> = config.lamp_command[6..]
            .chunks_exact(2)
            .map(|pair| {
                assert_eq!(pair[0], "--runtime-prefix-arg");
                pair[1].as_str()
            })
            .collect();
        assert_eq!(
            &prefix[..3],
            ["fake-clock-lamp-voice", "--fixture-control", control]
        );
        let old_prefix: Vec<_> = original[6..]
            .chunks_exact(2)
            .map(|pair| pair[1].as_str())
            .collect();
        assert_eq!(&prefix[3..], old_prefix);
        assert!(
            fixture.configure(&mut config).is_err(),
            "one driver cannot own two attempts"
        );
    }

    #[test]
    fn nonlocal_runtime_serve_and_recorder_configurations_are_refused_unchanged() {
        let directory = Private::new();
        let mut fixture = ClockedFixture::new(&directory.0).unwrap();
        let mut remote = config();
        remote.lamp_command[0] = "/usr/bin/ssh".into();
        let original = remote.lamp_command.clone();
        assert!(fixture.configure(&mut remote).is_err());
        assert_eq!(remote.lamp_command, original);
        let mut service = config();
        service.mode = LampMode::Serve {
            provider_config: "/unused/provider.json".into(),
        };
        assert!(fixture.configure(&mut service).is_err());
        let mut recorded = config();
        recorded.room_recorder = Some(vec!["/unused/recorder".into()]);
        assert!(fixture.configure(&mut recorded).is_err());
        let mut mismatched = config();
        mismatched.lamp_command[3] = "/unused/lamp-voice".into();
        assert!(fixture.configure(&mut mismatched).is_err());
        assert!(!fixture.configured);
    }

    #[test]
    fn clock_reads_are_pure_and_only_successful_room_sends_are_expected() {
        let directory = Private::new();
        let fixture = ClockedFixture::new(&directory.0).unwrap();
        let clock = fixture.clock();
        assert_eq!(clock.now().as_micros(), 0);
        assert_eq!(clock.now().as_micros(), 0);
        fixture.clock.advance(9_000).unwrap();
        assert_eq!(clock.now().as_micros(), 9_000);
        assert!(fixture.clock.advance(8_999).is_err());
        let room_path = directory.0.join("room.sock");
        let mut player = fixture.player(room_path.clone());
        let timing = SceneTiming {
            scene: "fixture-only".into(),
            duration_ms: 100,
            clips: Vec::new(),
            speech_span_ms: None,
            voices: 0,
            noise: false,
            source: TimingSource::TextEstimate,
        };
        let unused = Path::new("/unused");
        assert!(player.start(unused, &timing, unused, unused).is_err());
        assert_eq!(fixture.expected_room_inputs.get(), 0);
        let room = UnixDatagram::bind(room_path).unwrap();
        room.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let report = player
            .start(unused, &timing, unused, unused)
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(fixture.expected_room_inputs.get(), 1);
        assert_eq!(report["start_requested_host_monotonic_ns"], 9_000_000_u64);
        let mut bytes = [0; 2048];
        let count = room.recv(&mut bytes).unwrap();
        assert_eq!(
            serde_json::from_slice::<SceneTiming>(&bytes[..count]).unwrap(),
            timing
        );
    }

    #[test]
    fn frame_budget_and_missing_enqueue_fail_instead_of_skipping_output() {
        let capacity = FIXTURE_RELAY_QUEUE_CAPACITY as u64;
        ensure_queue_capacity(crate::physical::protocol::MAX_TRACE_LINES as u64, 0).unwrap();
        ensure_queue_capacity(capacity, 0).unwrap();
        ensure_queue_capacity(capacity * 2, capacity).unwrap();
        assert!(ensure_queue_capacity(capacity + 1, 0).is_err());
        assert!(ensure_queue_capacity(4, 5).is_err());
        let enqueued = AtomicU64::new(4);
        assert!(wait_for_enqueued(&enqueued, 5, Instant::now()).is_err());
        assert_eq!(enqueued.load(Ordering::Acquire), 4);
        wait_for_enqueued(&enqueued, 4, Instant::now()).unwrap();
    }

    #[test]
    fn startup_preserves_the_setup_ping_boundary_and_exit_rounds_freeze_time() {
        let directory = Private::new();
        let mut fixture = ClockedFixture::new(&directory.0).unwrap();
        fixture.configure(&mut config()).unwrap();
        let clock = fixture.clock();
        let path = fixture.control_path.clone();
        let enqueued = Arc::new(AtomicU64::new(0));
        let queued = Arc::clone(&enqueued);
        // These are protocol participants, not simulated product evidence.
        // Their exact rounds exercise only the parent's scheduling/barriers.
        let peers = thread::spawn(move || {
            let mut relay = Participant::connect(&path, Role::Relay).unwrap();
            assert_eq!(relay.current_step().at_us, BOOTSTRAP_US);
            let runtime = thread::spawn(move || {
                let mut runtime = Participant::connect(&path, Role::Runtime).unwrap();
                let first = runtime.current_step();
                assert_eq!(first.at_us, BOOTSTRAP_US);
                let next = runtime
                    .complete(Progress::Runtime {
                        poll_seq: 1,
                        stdout_frames: 1,
                        room_inputs: 0,
                        logical_end: false,
                    })
                    .unwrap();
                assert_eq!(next.at_us, BOOTSTRAP_US + POLL_US);
                runtime
                    .final_ack(Progress::Runtime {
                        poll_seq: 2,
                        stdout_frames: 1,
                        room_inputs: 0,
                        logical_end: true,
                    })
                    .unwrap();
            });
            let mut times = vec![relay.current_step().at_us];
            queued.store(2, Ordering::Release);
            let next = relay
                .complete(Progress::Relay {
                    poll_seq: 1,
                    stdout_frames: 0,
                    runner_inputs: 1,
                    output_frames: 2,
                    logical_end: false,
                })
                .unwrap();
            times.push(next.at_us);
            assert_eq!(next.stdout_target, 1);
            queued.store(3, Ordering::Release);
            let next = relay
                .complete(Progress::Relay {
                    poll_seq: 2,
                    stdout_frames: 1,
                    runner_inputs: 1,
                    output_frames: 3,
                    logical_end: false,
                })
                .unwrap();
            times.push(next.at_us);
            // Runtime logical completion alone is insufficient. One further
            // relay poll still has no terminal output and must not move time.
            let next = relay
                .complete(Progress::Relay {
                    poll_seq: 3,
                    stdout_frames: 1,
                    runner_inputs: 1,
                    output_frames: 3,
                    logical_end: false,
                })
                .unwrap();
            times.push(next.at_us);
            queued.store(4, Ordering::Release);
            relay
                .final_ack(Progress::Relay {
                    poll_seq: 4,
                    stdout_frames: 1,
                    runner_inputs: 1,
                    output_frames: 4,
                    logical_end: true,
                })
                .unwrap();
            runtime.join().unwrap();
            times
        });
        assert_eq!(
            clock.now().as_micros(),
            0,
            "initial Ping is sent before SessionStart"
        );
        fixture
            .wait(MonoTime::from_micros(10_000), 1, &enqueued, true)
            .unwrap();
        assert_eq!(clock.now().as_micros(), BOOTSTRAP_US);
        assert_eq!(enqueued.load(Ordering::Acquire), 3);
        fixture
            .wait(MonoTime::from_micros(10_000), 1, &enqueued, true)
            .unwrap();
        assert_eq!(clock.now().as_micros(), BOOTSTRAP_US + POLL_US);
        assert!(fixture.relay_done);
        assert_eq!(enqueued.load(Ordering::Acquire), 4);
        fixture
            .wait(MonoTime::from_micros(10_000), 1, &enqueued, true)
            .unwrap();
        assert_eq!(clock.now().as_micros(), 10_000);
        assert_eq!(peers.join().unwrap(), [2_000, 2_000, 4_000, 4_000]);
    }
}
