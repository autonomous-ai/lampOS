//! Explicit parent scheduler for one local serve fixture. Time advances only
//! between completed polls; real players, channels, signals and processes stay
//! in the path. An explicit delayed transport must complete its full wait and
//! actually exec the registered relay before that relay can start.
use super::{
    fixture_control::{DEFAULT_WATCHDOG, ParentListener, ParentPeer, Progress, Role, Step},
    fixture_driver::{
        RunnerFixtureClock, check_host_deadline, ensure_queue_capacity, next_round,
        wait_for_enqueued,
    },
    fixture_players::PreparedFixtureController,
    mac::{FakePlayerFault, FixtureDriver, LampMode, PhysicalConfig, Player},
};
use crate::{Result, invalid};
use lamp_turn_owner::MonoTime;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::Instant,
};

const POLL_US: u64 = 2_000;
/// The poll while a round has changed nothing: the fake runtime's own tick.
/// Over 99% of the eight-turn conversation's rounds change nothing, and the
/// test took more than twice as long at 2 ms throughout (#1806). After any
/// round that changes something, the next is the lamp's 2 ms again.
const IDLE_POLL_US: u64 = crate::fake::TICK_US;
// Retained startup readiness is 150 ms old. A positive epoch preserves that
// age instead of saturating it at zero. The setup Ping still precedes startup.
const EPOCH_US: u64 = 1_000_000;
const BOOTSTRAP_US: u64 = EPOCH_US + POLL_US;

struct RecorderControl {
    peer: ParentPeer,
    round: u64,
    wait_started_us: Option<u64>,
    exit_due_us: Option<u64>,
    logical_end: bool,
    exit_observed: bool,
}

struct SlowTransportControl {
    peer: ParentPeer,
    round: u64,
    wait_started_us: Option<u64>,
    exec_due_us: Option<u64>,
    logical_end: bool,
}

pub struct ClockedServeFixture {
    relay: Option<ParentPeer>,
    runtime: Option<ParentPeer>,
    recorder: Option<RecorderControl>,
    recorder_configured: bool,
    transport: Option<SlowTransportControl>,
    transport_delay_us: Option<u64>,
    transport_until: Option<Instant>,
    listener: Option<ParentListener>,
    control_path: PathBuf,
    clock: RunnerFixtureClock,
    players: PreparedFixtureController,
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
    stop_signal_sent_us: Option<u64>,
    await_sigterm: bool,
    terminal_until: Option<Instant>,
}

impl ClockedServeFixture {
    pub fn new(private_root: &Path) -> Result<Self> {
        let metadata = fs::symlink_metadata(private_root)?;
        if !private_root.is_absolute()
            || !metadata.is_dir()
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(invalid(
                "serve fixture needs an absolute caller-owned private directory",
            ));
        }
        let control_path = private_root.join("serve-fixture.sock");
        let listener = ParentListener::bind(&control_path)?;
        let clock = RunnerFixtureClock::new(EPOCH_US);
        let player_clock = clock.clone();
        let players = PreparedFixtureController::new(move || player_clock.now());
        Ok(Self {
            relay: None,
            runtime: None,
            recorder: None,
            recorder_configured: false,
            transport: None,
            transport_delay_us: None,
            transport_until: None,
            listener: Some(listener),
            control_path,
            clock,
            players,
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
            stop_signal_sent_us: None,
            await_sigterm: false,
            terminal_until: None,
        })
    }

    pub fn clock(&self) -> RunnerFixtureClock {
        self.clock.clone()
    }

    pub fn player(
        &self,
        room: PathBuf,
        faults: Vec<(usize, FakePlayerFault)>,
    ) -> impl Player + use<> {
        self.players.player(room, faults)
    }

    /// Preserve every fake argument and actual local child. This scheduler
    /// cannot opt a real runtime, a shell wrapper or a real recorder into its clock.
    /// Only the same evaluator's explicit slow-transport wrapper can opt in.
    pub fn configure(&mut self, config: &mut PhysicalConfig) -> Result<()> {
        if self.configured || self.started {
            return Err(invalid(
                "serve fixture can configure only one fresh attempt",
            ));
        }
        if !matches!(config.mode, LampMode::Serve { .. }) {
            return Err(invalid("serve fixture requires serve mode"));
        }
        let (lamp_command, transport_delay_us) =
            controlled_transport(&config.lamp_command, &self.control_path)?;
        let room_recorder = config
            .room_recorder
            .as_deref()
            .map(|command| {
                controlled_recorder(command, &config.lamp_command[0], &self.control_path)
            })
            .transpose()?;
        self.recorder_configured = room_recorder.is_some();
        self.transport_delay_us = transport_delay_us;
        config.lamp_command = lamp_command;
        config.room_recorder = room_recorder;
        self.configured = true;
        Ok(())
    }

    fn close(&mut self) {
        self.players.close();
        drop(self.runtime.take());
        drop(self.relay.take());
        drop(self.recorder.take());
        drop(self.transport.take());
        drop(self.listener.take());
    }

    fn recorder_pending(&self) -> bool {
        self.recorder
            .as_ref()
            .is_some_and(|recorder| recorder.logical_end && !recorder.exit_observed)
    }

    fn require_recorder_exit_observed(&self) -> Result<()> {
        if self.recorder_pending() {
            return Err(invalid(
                "recorder terminal acknowledgement still needs actual child exit",
            ));
        }
        Ok(())
    }

    fn recorder_deadline(&self, proposed: u64) -> u64 {
        self.recorder
            .as_ref()
            .filter(|recorder| !recorder.logical_end)
            .and_then(|recorder| recorder.exit_due_us)
            .map_or(proposed, |due| proposed.min(due))
    }

    fn recorder_step(&mut self, host_until: Instant) -> Result<()> {
        let Some(recorder) = &mut self.recorder else {
            return if self.recorder_configured {
                Err(invalid("configured recorder has not registered"))
            } else {
                Ok(())
            };
        };
        if recorder.logical_end {
            return Ok(());
        }
        let round = next_round(recorder.round)?;
        let progress = recorder.peer.step_until(
            Step {
                round,
                at_us: self.clock.now().as_micros(),
                stdout_target: 0,
                runner_input_target: 0,
                wait_for_sigterm: false,
            },
            host_until,
        )?;
        let Progress::Recorder {
            ready_written,
            wait_started_us,
            exit_due_us,
            logical_end,
            ..
        } = progress
        else {
            return Err(invalid("recorder returned another role's progress"));
        };
        if !ready_written
            || recorder
                .wait_started_us
                .is_some_and(|previous| previous != wait_started_us)
            || recorder
                .exit_due_us
                .is_some_and(|previous| previous != exit_due_us)
        {
            return Err(invalid(
                "recorder changed its completed ready/wait boundary",
            ));
        }
        recorder.round = round;
        recorder.wait_started_us = Some(wait_started_us);
        recorder.exit_due_us = Some(exit_due_us);
        recorder.logical_end = logical_end;
        Ok(())
    }

    fn advance_with_recorder(&mut self, at_us: u64, host_until: Instant) -> Result<()> {
        self.require_recorder_exit_observed()?;
        self.clock.advance(at_us)?;
        self.recorder_step(host_until)?;
        self.players.pump_before(host_until)
    }

    fn start_recorder_inner(&mut self, host_until: Instant) -> Result<()> {
        if !self.configured
            || !self.recorder_configured
            || self.started
            || self.failed
            || self.recorder.is_some()
            || self.clock.now().as_micros() != EPOCH_US
        {
            return Err(invalid(
                "recorder startup requires its fresh configured epoch",
            ));
        }
        let peer = self
            .listener
            .as_ref()
            .ok_or_else(|| invalid("serve listener is closed"))?
            .accept_until(Role::Recorder, host_until)?;
        self.recorder = Some(RecorderControl {
            peer,
            round: 0,
            wait_started_us: None,
            exit_due_us: None,
            logical_end: false,
            exit_observed: false,
        });
        // The child really writes ready.json before acknowledging this Step.
        // The backend still reads that file and owns every actual exit check.
        self.recorder_step(host_until)
    }

    fn runtime_step(&mut self, until: Instant) -> Result<()> {
        let round = next_round(self.runtime_round)?;
        let progress = self
            .runtime
            .as_mut()
            .ok_or_else(|| invalid("serve runtime is not connected"))?
            .step_until(
                Step {
                    round,
                    at_us: self.clock.now().as_micros(),
                    stdout_target: 0,
                    runner_input_target: 0,
                    wait_for_sigterm: self.await_sigterm,
                },
                until,
            )?;
        let Progress::ServeRuntime {
            stdout_frames,
            room_consumed,
            observed_signal,
            logical_end,
            ..
        } = progress
        else {
            return Err(invalid("serve runtime returned another role's progress"));
        };
        let sent = self.players.successful_sends();
        // An original stop check precedes the original room drain. A terminal
        // poll may leave sent inputs unconsumed; never announce them as read.
        if room_consumed > sent || (!logical_end && room_consumed != sent) {
            return Err(invalid(&format!(
                "serve runtime consumed {room_consumed} datagrams after {sent} actual sends"
            )));
        }
        if self.await_sigterm && observed_signal != signal_hook::consts::SIGTERM as usize {
            return Err(invalid(
                "serve runtime did not observe the delivered SIGTERM",
            ));
        }
        self.await_sigterm = false;
        self.runtime_round = round;
        self.runtime_stdout = stdout_frames;
        self.runtime_rooms = room_consumed;
        self.runtime_done = logical_end;
        if logical_end {
            self.terminal_until = Some(until);
        }
        Ok(())
    }

    fn relay_step(&mut self, sent: u64, enqueued: &AtomicU64, until: Instant) -> Result<()> {
        let round = next_round(self.relay_round)?;
        let progress = self
            .relay
            .as_mut()
            .ok_or_else(|| invalid("serve relay is not connected"))?
            .step_until(
                Step {
                    round,
                    at_us: self.clock.now().as_micros(),
                    stdout_target: self.runtime_stdout,
                    runner_input_target: sent,
                    wait_for_sigterm: false,
                },
                until,
            )?;
        let Progress::ServeRelay {
            stdout_frames,
            runner_inputs,
            output_frames,
            stop_signal_sent_us,
            logical_end,
            ..
        } = progress
        else {
            return Err(invalid("serve relay returned another role's progress"));
        };
        if stdout_frames != self.runtime_stdout || runner_inputs != sent {
            return Err(invalid(&format!(
                "serve relay consumed stdout/input {stdout_frames}/{runner_inputs}, expected {}/{sent}",
                self.runtime_stdout
            )));
        }
        if output_frames < self.output_frames {
            return Err(invalid("serve relay output count regressed"));
        }
        ensure_queue_capacity(output_frames, self.drained_frames)?;
        wait_for_enqueued(enqueued, output_frames, until)?;
        if stop_signal_sent_us != self.stop_signal_sent_us {
            if self.stop_signal_sent_us.is_some() || stop_signal_sent_us.is_none() {
                return Err(invalid(
                    "serve relay changed its actual signal-send receipt",
                ));
            }
            self.stop_signal_sent_us = stop_signal_sent_us;
            self.await_sigterm = !self.runtime_done;
        }
        self.relay_round = round;
        self.runner_inputs = runner_inputs;
        self.output_frames = output_frames;
        self.relay_done = logical_end;
        if logical_end {
            // The relay's terminal wrapper already completed actual cleanup
            // and output. In the forced-kill case no further runtime poll is
            // possible or needed; EOF alone would not establish this state.
            drop(self.runtime.take());
        }
        Ok(())
    }

    fn finish_runtime_exit(&mut self, sent: u64, enqueued: &AtomicU64) -> Result<()> {
        if !self.runtime_done || self.relay_done {
            return Ok(());
        }
        let until = self
            .terminal_until
            .ok_or_else(|| invalid("missing serve terminal host deadline"))?;
        while !self.relay_done {
            check_host_deadline(until, "serve child exit and final relay output")?;
            self.relay_step(sent, enqueued, until)?;
            thread::yield_now();
        }
        Ok(())
    }

    fn transport_step(&mut self, until: Instant) -> Result<()> {
        let delay = self
            .transport_delay_us
            .ok_or_else(|| invalid("slow transport was not configured"))?;
        let transport = self
            .transport
            .as_mut()
            .ok_or_else(|| invalid("slow transport has not registered"))?;
        if transport.logical_end {
            return Ok(());
        }
        let round = next_round(transport.round)?;
        let at_us = self.clock.now().as_micros();
        let progress = transport.peer.step_until(
            Step {
                round,
                at_us,
                stdout_target: 0,
                runner_input_target: 0,
                wait_for_sigterm: false,
            },
            until,
        )?;
        let Progress::SlowTransport {
            wait_started_us,
            exec_due_us,
            logical_end,
            ..
        } = progress
        else {
            return Err(invalid("slow transport returned another role's progress"));
        };
        let expected_due = EPOCH_US
            .checked_add(delay)
            .ok_or_else(|| invalid("slow transport deadline overflow"))?;
        if wait_started_us != EPOCH_US
            || exec_due_us != expected_due
            || logical_end != (at_us >= exec_due_us)
            || transport
                .wait_started_us
                .is_some_and(|previous| previous != wait_started_us)
            || transport
                .exec_due_us
                .is_some_and(|previous| previous != exec_due_us)
        {
            return Err(invalid("slow transport changed its full wait boundary"));
        }
        transport.round = round;
        transport.wait_started_us = Some(wait_started_us);
        transport.exec_due_us = Some(exec_due_us);
        transport.logical_end = logical_end;
        Ok(())
    }

    fn wait_slow_transport(
        &mut self,
        logical_until: u64,
        sent: u64,
        enqueued: &AtomicU64,
        until: Instant,
    ) -> Result<()> {
        if self.transport.is_none() {
            if self.clock.now().as_micros() != EPOCH_US {
                return Err(invalid("slow transport must register at the held epoch"));
            }
            let peer = self
                .listener
                .as_ref()
                .ok_or_else(|| invalid("serve listener is closed"))?
                .accept_until(Role::SlowTransport, until)?;
            self.transport = Some(SlowTransportControl {
                peer,
                round: 0,
                wait_started_us: None,
                exec_due_us: None,
                logical_end: false,
            });
            self.transport_step(until)?;
        }
        let due = self
            .transport
            .as_ref()
            .and_then(|transport| transport.exec_due_us)
            .ok_or_else(|| invalid("slow transport has no completed initial wait check"))?;
        if !self.transport.as_ref().unwrap().logical_end {
            let next = self
                .clock
                .now()
                .as_micros()
                .checked_add(POLL_US)
                .ok_or_else(|| invalid("slow transport poll time overflow"))?;
            let next = self.recorder_deadline(logical_until.min(next).min(due));
            self.advance_with_recorder(next, until)?;
            self.transport_step(until)?;
        }
        if !self.transport.as_ref().unwrap().logical_end {
            return Ok(());
        }
        // A delay Ack does not establish exec. Hold this exact instant until
        // the newly executed controlled relay registers. It has not consumed
        // any queued runner input or produced its first session poll yet.
        if self.relay.is_none() {
            self.relay = Some(
                self.listener
                    .as_ref()
                    .ok_or_else(|| invalid("serve listener is closed"))?
                    .accept_until(Role::ServeRelay, until)?,
            );
        }
        if self.recorder_pending() {
            return Ok(());
        }
        // Preserve the ordinary setup-Ping separation for a zero or sub-poll
        // delay. A real relay registration precedes this possible advance.
        let bootstrap = due.max(BOOTSTRAP_US);
        let at = self.clock.now().as_micros();
        if at < bootstrap {
            let next = at
                .checked_add(POLL_US)
                .ok_or_else(|| invalid("slow transport bootstrap time overflow"))?;
            let next = self.recorder_deadline(logical_until.min(next).min(bootstrap));
            self.advance_with_recorder(next, until)?;
            if self.recorder_pending() || next < bootstrap {
                return Ok(());
            }
        }
        self.bootstrap_at(bootstrap, sent, enqueued, until)
    }

    fn bootstrap(&mut self, sent: u64, enqueued: &AtomicU64, until: Instant) -> Result<()> {
        self.bootstrap_at(BOOTSTRAP_US, sent, enqueued, until)
    }

    fn bootstrap_at(
        &mut self,
        at_us: u64,
        sent: u64,
        enqueued: &AtomicU64,
        until: Instant,
    ) -> Result<()> {
        self.advance_with_recorder(at_us, until)?;
        if self.recorder_pending() {
            return Ok(());
        }
        if self.relay.is_none() {
            self.relay = Some(
                self.listener
                    .as_ref()
                    .ok_or_else(|| invalid("serve listener is closed"))?
                    .accept_until(Role::ServeRelay, until)?,
            );
        }
        self.relay_step(sent, enqueued, until)?;
        if self.relay_done {
            return Err(invalid("serve relay ended before runtime registration"));
        }
        self.runtime = Some(
            self.listener
                .as_ref()
                .ok_or_else(|| invalid("serve listener is closed"))?
                .accept_until(Role::ServeRuntime, until)?,
        );
        self.runtime_step(until)?;
        // Room readiness is an actual filesystem observation by the owned
        // preparer. Pump it again at this same instant after runtime startup.
        self.players.pump_before(until)?;
        self.relay_step(sent, enqueued, until)?;
        self.started = true;
        self.transport_until = None;
        self.next_poll_us = at_us
            .checked_add(POLL_US)
            .ok_or_else(|| invalid("serve bootstrap poll time overflow"))?;
        self.finish_runtime_exit(sent, enqueued)
    }

    fn wait_inner(
        &mut self,
        logical_until: MonoTime,
        sent: u64,
        enqueued: &AtomicU64,
        queue_drained: bool,
        host_until: Instant,
    ) -> Result<()> {
        if !self.configured || self.failed {
            return Err(invalid("serve fixture is unconfigured or failed"));
        }
        check_host_deadline(host_until, "serve progress")?;
        self.require_recorder_exit_observed()?;
        self.players.settle_before(host_until)?;
        if sent < self.runner_inputs || self.players.successful_sends() < self.runtime_rooms {
            return Err(invalid("serve producer counters regressed"));
        }
        if queue_drained {
            self.drained_frames = enqueued.load(Ordering::Acquire);
        }
        let at = self.clock.now().as_micros();
        let until = logical_until.as_micros();
        if !self.started && self.transport_delay_us.is_some() && until > at {
            return self.wait_slow_transport(until, sent, enqueued, host_until);
        }
        if until <= at {
            return Ok(());
        }
        if !self.started {
            let next = self.recorder_deadline(until.min(BOOTSTRAP_US));
            if next < BOOTSTRAP_US {
                return self.advance_with_recorder(next, host_until);
            }
            return self.bootstrap(sent, enqueued, host_until);
        }
        if self.relay_done {
            // A relay's end does not complete a prepared player's pending
            // delay. Preserve its polling cadence through actual completion.
            let next = at
                .checked_add(POLL_US)
                .ok_or_else(|| invalid("serve player poll time overflow"))?;
            return self.advance_with_recorder(self.recorder_deadline(until.min(next)), host_until);
        }
        if self.runtime_done {
            return self.finish_runtime_exit(sent, enqueued);
        }
        let next = self.recorder_deadline(until.min(self.next_poll_us));
        self.advance_with_recorder(next, host_until)?;
        // Let the backend observe its real recorder Child at this held instant.
        // If a runtime poll is also due, its original deadline stays pending
        // for the next same-instant exchange; no poll is added or skipped.
        if self.recorder_pending() || next < self.next_poll_us {
            return Ok(());
        }
        let before = self.progress();
        self.runtime_step(host_until)?;
        self.players.pump_before(host_until)?;
        self.relay_step(sent, enqueued, host_until)?;
        let step = if self.progress() == before {
            IDLE_POLL_US
        } else {
            POLL_US
        };
        self.next_poll_us = next
            .checked_add(step)
            .ok_or_else(|| invalid("serve poll time overflow"))?;
        self.finish_runtime_exit(sent, enqueued)
    }

    /// What a round can change: the runtime's output and rooms read, the
    /// relay's inputs and output, and the players' sends.
    fn progress(&self) -> (u64, u64, u64, u64, u64) {
        (
            self.runtime_stdout,
            self.runtime_rooms,
            self.runner_inputs,
            self.output_frames,
            self.players.successful_sends(),
        )
    }

    fn wait_before(
        &mut self,
        until: MonoTime,
        sent: u64,
        enqueued: &AtomicU64,
        queue_drained: bool,
        host_until: Instant,
    ) -> Result<()> {
        let mut host_until = host_until.min(Instant::now() + DEFAULT_WATCHDOG);
        if self.transport_delay_us.is_some() && !self.started {
            // One delayed startup owns one host budget across every logical
            // step, including the real exec and relay/runtime registration.
            host_until = host_until.min(*self.transport_until.get_or_insert(host_until));
            self.transport_until = Some(host_until);
        }
        let result = self.wait_inner(until, sent, enqueued, queue_drained, host_until);
        if result.is_err() {
            self.failed = true;
            self.close();
        }
        result
    }
}

impl FixtureDriver for ClockedServeFixture {
    fn supports_serve(&self) -> bool {
        true
    }

    fn supports_recorder(&self) -> bool {
        self.configured && self.recorder_configured
    }

    fn start_recorder(&mut self, host_deadline: Instant) -> Result<()> {
        let result = self.start_recorder_inner(host_deadline);
        if result.is_err() {
            self.failed = true;
            self.close();
        }
        result
    }

    fn recorder_exit_pending(&self) -> bool {
        self.recorder_pending()
    }

    fn recorder_exit_observed(&mut self) -> Result<()> {
        let Some(recorder) = &mut self.recorder else {
            return Err(invalid("no registered recorder exit to observe"));
        };
        if !recorder.logical_end || recorder.exit_observed {
            return Err(invalid(
                "recorder exit observation has no pending terminal Ack",
            ));
        }
        // Only the backend calls this after Child::try_wait returned a status.
        // No status value or recording evidence is supplied by this protocol.
        recorder.exit_observed = true;
        Ok(())
    }

    fn finish_recorder(&mut self, host_deadline: Instant) -> Result<()> {
        if !self.recorder_configured {
            return Ok(());
        }
        let result = (|| {
            if self.failed || !self.relay_done || self.recorder.is_none() {
                return Err(invalid("recorder drain requires a completed serve relay"));
            }
            self.players.settle_before(host_deadline)?;
            while !self.recorder.as_ref().unwrap().logical_end {
                check_host_deadline(host_deadline, "serve recorder completion")?;
                let next = self
                    .clock
                    .now()
                    .as_micros()
                    .checked_add(POLL_US)
                    .ok_or_else(|| invalid("serve recorder completion time overflow"))?;
                self.advance_with_recorder(self.recorder_deadline(next), host_deadline)?;
            }
            // Return to the backend without another clock advance. Its actual
            // Child exit barrier precedes every remaining player finish wait.
            Ok(())
        })();
        if result.is_err() {
            self.failed = true;
            self.close();
        }
        result
    }

    fn grant_sent(&mut self, attempt_id: &str) -> Result<()> {
        let result = self.players.grant_sent(attempt_id);
        if result.is_err() {
            self.failed = true;
            self.close();
        }
        result
    }

    fn player_report_ready(&self, attempt_id: &str) -> bool {
        self.players.report_ready(attempt_id)
    }

    fn finish_players(&mut self, host_deadline: Instant) -> Result<()> {
        let result = (|| {
            self.require_recorder_exit_observed()?;
            if self.recorder_configured
                && !self
                    .recorder
                    .as_ref()
                    .is_some_and(|recorder| recorder.exit_observed)
            {
                return Err(invalid("players cannot finish ahead of the owned recorder"));
            }
            self.players.settle_before(host_deadline)?;
            while !self.players.all_reports_ready() {
                check_host_deadline(host_deadline, "serve prepared-player completion")?;
                let next = self
                    .clock
                    .now()
                    .as_micros()
                    .checked_add(POLL_US)
                    .ok_or_else(|| invalid("serve player completion time overflow"))?;
                self.clock.advance(next)?;
                self.players.pump_before(host_deadline)?;
            }
            Ok(())
        })();
        if result.is_err() {
            self.failed = true;
            self.close();
        }
        result
    }

    fn wait(
        &mut self,
        until: MonoTime,
        sent: u64,
        enqueued: &AtomicU64,
        queue_drained: bool,
    ) -> Result<()> {
        self.wait_before(
            until,
            sent,
            enqueued,
            queue_drained,
            Instant::now() + DEFAULT_WATCHDOG,
        )
    }

    fn prepare_wait(
        &mut self,
        until: MonoTime,
        sent: u64,
        enqueued: &AtomicU64,
        queue_drained: bool,
        host_deadline: Instant,
    ) -> Result<()> {
        self.wait_before(until, sent, enqueued, queue_drained, host_deadline)
    }
}

impl Drop for ClockedServeFixture {
    fn drop(&mut self) {
        self.close();
    }
}

fn controlled_transport(
    command: &[String],
    control_path: &Path,
) -> Result<(Vec<String>, Option<u64>)> {
    if command.get(1).map(String::as_str) != Some("fake-slow-transport") {
        return controlled_command(command, control_path).map(|command| (command, None));
    }
    if command.get(2).map(String::as_str) != Some("--delay-ms")
        || command.get(4).map(String::as_str) != Some("--")
        || command.get(5) != command.first()
    {
        return Err(invalid(
            "serve fixture accepts only its evaluator's explicit slow transport",
        ));
    }
    let delay = command
        .get(3)
        .ok_or_else(|| invalid("slow transport is missing its delay"))?;
    let delay_us = delay
        .parse::<u64>()?
        .checked_mul(1_000)
        .ok_or_else(|| invalid("slow transport delay exceeds the fixture clock"))?;
    EPOCH_US
        .checked_add(delay_us)
        .and_then(|due| due.checked_mul(1_000))
        .ok_or_else(|| invalid("slow transport deadline exceeds the shared clock domain"))?;
    let relay = controlled_command(&command[5..], control_path)?;
    let control = control_path
        .to_str()
        .ok_or_else(|| invalid("serve control path must be UTF-8"))?;
    let mut configured = vec![
        command[0].clone(),
        "fake-clock-slow-transport".into(),
        "--fixture-control".into(),
        control.into(),
        "--delay-ms".into(),
        delay.clone(),
        "--".into(),
    ];
    configured.extend(relay);
    Ok((configured, Some(delay_us)))
}

fn controlled_command(command: &[String], control_path: &Path) -> Result<Vec<String>> {
    let executable = command
        .first()
        .ok_or_else(|| invalid("missing evaluator command"))?;
    let path = Path::new(executable);
    if !path.is_absolute()
        || path.file_name().and_then(|name| name.to_str()) != Some("lamp-voice-eval")
        || command.get(1).map(String::as_str) != Some("lamp-session")
    {
        return Err(invalid(
            "serve fixture requires a direct local evaluator relay",
        ));
    }
    let mut runtime = None;
    let mut prefix = Vec::new();
    let mut index = 2;
    while index < command.len() {
        let value = command
            .get(index + 1)
            .ok_or_else(|| invalid("incomplete serve fixture option"))?;
        match command[index].as_str() {
            "--runtime" if runtime.is_none() => runtime = Some(value),
            "--runtime-prefix-arg" => prefix.push(value.clone()),
            _ => {
                return Err(invalid(
                    "serve fixture accepts one runtime and explicit prefix arguments",
                ));
            }
        }
        index += 2;
    }
    if runtime != Some(executable) || prefix.first().map(String::as_str) != Some("fake-lamp-voice")
    {
        return Err(invalid(
            "serve fixture requires the same evaluator's fake runtime",
        ));
    }
    if prefix.len() % 2 != 1
        || prefix[1..]
            .chunks_exact(2)
            .any(|pair| !pair[0].starts_with("--fake-"))
    {
        return Err(invalid(
            "serve fixture prefix needs --fake-* option/value pairs",
        ));
    }
    let control = control_path
        .to_str()
        .ok_or_else(|| invalid("serve control path must be UTF-8"))?;
    let mut configured = vec![
        executable.clone(),
        "fake-clock-serve-session".into(),
        "--fixture-control".into(),
        control.into(),
        "--runtime".into(),
        executable.clone(),
    ];
    for value in ["fake-clock-serve-runtime", "--fixture-control", control] {
        configured.extend(["--runtime-prefix-arg".into(), value.into()]);
    }
    for value in &prefix[1..] {
        configured.extend(["--runtime-prefix-arg".into(), value.clone()]);
    }
    Ok(configured)
}

fn controlled_recorder(
    command: &[String],
    evaluator: &str,
    control_path: &Path,
) -> Result<Vec<String>> {
    if command.first().map(String::as_str) != Some(evaluator)
        || command.get(1).map(String::as_str) != Some("fake-room-recorder")
        || !command.len().is_multiple_of(2)
        || command[2..].chunks_exact(2).any(|pair| {
            !matches!(
                pair[0].as_str(),
                "--out"
                    | "--seconds"
                    | "--fail-after-ms"
                    | "--exit-code"
                    | "--declare-end-after-ms"
            )
        })
    {
        return Err(invalid(
            "serve fixture requires the same evaluator's direct fake room recorder",
        ));
    }
    let control = control_path
        .to_str()
        .ok_or_else(|| invalid("serve control path must be UTF-8"))?;
    let mut configured = vec![
        evaluator.into(),
        "fake-clock-room-recorder".into(),
        "--fixture-control".into(),
        control.into(),
    ];
    // Keep the actual output/duration placeholders and every original argument.
    // The shared recorder parser still decides their values and return code.
    configured.extend_from_slice(&command[2..]);
    Ok(configured)
}

#[cfg(test)]
#[path = "recorder_fixture_driver_tests.rs"]
mod recorder_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn command() -> Vec<String> {
        [
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
            "--runtime-prefix-arg",
            "--fake-ignore-sigterm",
            "--runtime-prefix-arg",
            "yes",
        ]
        .map(str::to_owned)
        .into()
    }

    #[test]
    fn configuration_keeps_every_fake_argument_and_selects_only_explicit_commands() {
        let original = command();
        let changed = controlled_command(&original, Path::new("/private/control.sock")).unwrap();
        assert_eq!(
            &changed[..6],
            [
                "/unused/lamp-voice-eval",
                "fake-clock-serve-session",
                "--fixture-control",
                "/private/control.sock",
                "--runtime",
                "/unused/lamp-voice-eval"
            ]
        );
        assert_eq!(
            &changed[6..12],
            [
                "--runtime-prefix-arg",
                "fake-clock-serve-runtime",
                "--runtime-prefix-arg",
                "--fixture-control",
                "--runtime-prefix-arg",
                "/private/control.sock"
            ]
        );
        assert_eq!(&changed[12..], &original[6..]);
    }

    #[test]
    fn shell_transports_real_runtimes_and_malformed_prefixes_cannot_opt_in() {
        assert!(controlled_command(&[], Path::new("/private/control.sock")).is_err());
        for (index, value) in [
            (0, "lamp-voice-eval"),
            (1, "fake-slow-transport"),
            (3, "/unused/lamp-live"),
            (5, "serve"),
            (7, "--real-room"),
        ] {
            let mut bad = command();
            bad[index] = value.into();
            assert!(
                controlled_command(&bad, Path::new("/private/control.sock")).is_err(),
                "{bad:?}"
            );
        }
        let mut bad = command();
        bad.pop();
        assert!(controlled_command(&bad, Path::new("/private/control.sock")).is_err());
    }

    #[test]
    fn positive_pure_epoch_preserves_retained_history_and_setup_ping_order() {
        let clock = RunnerFixtureClock::new(EPOCH_US);
        let before = clock.now();
        assert_eq!(clock.now(), before);
        assert_eq!(clock.now(), before);
        clock.advance(BOOTSTRAP_US).unwrap();
        assert!(before < clock.now());
        assert_eq!(
            clock.now().as_micros().saturating_sub(150_000),
            BOOTSTRAP_US - 150_000
        );
        assert!(clock.advance(EPOCH_US).is_err());
        assert_eq!(clock.now().as_micros(), BOOTSTRAP_US);
    }
}

#[cfg(test)]
#[path = "slow_transport_fixture_driver_tests.rs"]
mod slow_transport_tests;
