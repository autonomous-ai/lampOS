//! A current-thread coordinator for the six relay fixtures. Session owns the
//! real child, readers, SIGTERM, kill, reap, metadata and output. This helper
//! owns only its real runner writer and the explicitly controlled fake peer.
use super::{BIN, Runner};
use lamp_turn_owner::MonoTime;
use lamp_voice_eval::{
    Result, invalid,
    physical::{
        fixture_control::{ParentListener, ParentPeer, Progress, Role, Step},
        session::{self, RelayPollProgress, RelayPollTargets, SessionMode, SessionOptions},
    },
};
use std::{
    cell::{Cell, RefCell},
    io::{self, Write},
    os::unix::net::UnixStream,
    path::Path,
    thread,
    time::{Duration, Instant},
};

const EPOCH_US: u64 = 1_000_000;
const POLL_US: u64 = 2_000;
const HOST_GUARD: Duration = Duration::from_secs(30);
const STOP_LINE: &[u8] = b"{\"type\":\"stop\",\"reason\":\"test\"}\n";

#[derive(Clone, Copy)]
enum Action {
    Stop,
    Close,
}

struct Control {
    listener: ParentListener,
    runtime: Option<ParentPeer>,
    runner: Option<UnixStream>,
    action: Option<(u64, Action)>,
    hard_due_us: u64,
    runtime_round: u64,
    runtime_at: Option<u64>,
    runtime_ended: bool,
    stdout_frames: u64,
    runner_frames: u64,
    relay_poll: u64,
    signal_sent_us: Option<u64>,
    observe_signal: bool,
    pending_stop_until: Option<Instant>,
    stop_until: Option<Instant>,
    terminal_until: Option<Instant>,
    relay_ended: bool,
}

impl Control {
    fn new(
        listener: ParentListener,
        writer: UnixStream,
        runner: Runner,
        hard_due_us: u64,
    ) -> Result<Self> {
        writer.set_write_timeout(Some(HOST_GUARD))?;
        let action = match runner {
            Runner::StopAfter(after) => Some((after, Action::Stop)),
            Runner::CloseAfter(after) => Some((after, Action::Close)),
            Runner::Silent => None,
        }
        .map(|(after, action)| {
            let micros = u64::try_from(after.as_micros())?;
            if Duration::from_micros(micros) != after {
                return Err(invalid("relay runner delay must retain exact microseconds"));
            }
            let due = EPOCH_US
                .checked_add(micros)
                .ok_or_else(|| invalid("relay runner delay overflow"))?;
            Ok((due, action))
        })
        .transpose()?;
        Ok(Self {
            listener,
            runtime: None,
            runner: Some(writer),
            action,
            hard_due_us,
            runtime_round: 0,
            runtime_at: None,
            runtime_ended: false,
            stdout_frames: 0,
            runner_frames: 0,
            relay_poll: 0,
            signal_sent_us: None,
            observe_signal: false,
            pending_stop_until: None,
            stop_until: None,
            terminal_until: None,
            relay_ended: false,
        })
    }

    fn host_until(&self) -> Result<Instant> {
        let now = Instant::now();
        let mut until = now + HOST_GUARD;
        for boundary in [
            self.pending_stop_until,
            self.stop_until,
            self.terminal_until,
        ]
        .into_iter()
        .flatten()
        {
            until = until.min(boundary);
        }
        if until <= now {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "serve relay fixture exceeded its held host completion bound",
            )
            .into());
        }
        Ok(until)
    }

    fn fire_runner(&mut self, at_us: u64) -> Result<()> {
        let Some((due, action)) = self.action else {
            return Ok(());
        };
        if at_us < due {
            return Ok(());
        }
        match action {
            Action::Stop => {
                self.runner
                    .as_mut()
                    .ok_or_else(|| invalid("runner writer closed before its Stop"))?
                    .write_all(STOP_LINE)?;
                self.runner_frames += 1;
            }
            Action::Close => drop(self.runner.take()),
        }
        self.action = None;
        self.pending_stop_until = Some(Instant::now() + HOST_GUARD);
        Ok(())
    }

    fn before(&mut self, at_us: u64) -> Result<RelayPollTargets> {
        let until = self.host_until()?;
        if self.relay_ended {
            return Err(invalid("serve relay polled after its terminal receipt"));
        }
        self.fire_runner(at_us)?;
        if self.runtime.is_none() {
            self.runtime = Some(self.listener.accept_until(Role::ServeRuntime, until)?);
        }
        if !self.runtime_ended && (self.runtime_at != Some(at_us) || self.observe_signal) {
            let round = self
                .runtime_round
                .checked_add(1)
                .ok_or_else(|| invalid("serve relay fixture round overflow"))?;
            let progress = self
                .runtime
                .as_mut()
                .expect("runtime accepted")
                .step_until(
                    Step {
                        round,
                        at_us,
                        stdout_target: 0,
                        runner_input_target: 0,
                        wait_for_sigterm: self.observe_signal,
                    },
                    until,
                )?;
            let Progress::ServeRuntime {
                stdout_frames,
                observed_signal,
                logical_end,
                ..
            } = progress
            else {
                return Err(invalid("serve relay received another runtime role"));
            };
            if self.observe_signal && observed_signal != signal_hook::consts::SIGTERM as usize {
                return Err(invalid("serve runtime did not observe the actual SIGTERM"));
            }
            self.runtime_round = round;
            self.runtime_at = Some(at_us);
            self.stdout_frames = stdout_frames;
            self.runtime_ended = logical_end;
            self.observe_signal = false;
            if logical_end {
                // This is a final-output receipt, never an OS-exit receipt.
                self.terminal_until = Some(until);
            }
        }
        Ok(RelayPollTargets {
            stdout_frames: self.stdout_frames,
            runner_inputs: self.runner_frames,
        })
    }

    fn completed(&mut self, clock: &Cell<u64>, poll: RelayPollProgress) -> Result<()> {
        if self.relay_ended || self.relay_poll.checked_add(1) != Some(poll.poll_seq) {
            return Err(invalid("serve relay completion sequence changed"));
        }
        if poll.stdout_frames < self.stdout_frames || poll.runner_inputs < self.runner_frames {
            return Err(invalid(
                "serve relay completed before consuming announced frames",
            ));
        }
        self.relay_poll = poll.poll_seq;
        self.host_until()?;
        if poll.terminal {
            // Session already observed Child::try_wait or performed its real
            // grace-expiry kill/wait. Its final drains and metadata still run.
            self.relay_ended = true;
            return Ok(());
        }
        let at_us = clock.get();
        if poll.stop_signal_sent_us != self.signal_sent_us {
            if self.signal_sent_us.is_some() || poll.stop_signal_sent_us != Some(at_us) {
                return Err(invalid("serve relay changed its real stop signal receipt"));
            }
            self.signal_sent_us = poll.stop_signal_sent_us;
            self.pending_stop_until = None;
            self.stop_until =
                Some(Instant::now() + Duration::from_micros(session::SERVE_STOP_GRACE_US));
            self.observe_signal = true;
            return Ok(());
        }
        if self.pending_stop_until.is_some() || self.runtime_ended {
            // EOF enqueue and OS exit are independently scheduled real work.
            // Keep both clocks fixed and reuse the parked runtime's receipt.
            thread::yield_now();
            return Ok(());
        }
        let mut next = at_us
            .checked_add(POLL_US)
            .ok_or_else(|| invalid("serve relay fixture time overflow"))?;
        for due in [
            self.action.map(|(due, _)| due),
            Some(self.hard_due_us),
            self.signal_sent_us
                .and_then(|at| at.checked_add(session::SERVE_STOP_GRACE_US)),
        ]
        .into_iter()
        .flatten()
        {
            if due > at_us {
                next = next.min(due);
            }
        }
        clock.set(next);
        Ok(())
    }
}

impl Drop for Control {
    fn drop(&mut self) {
        // The session's own Cleanup owns kill/reap. Closing these endpoints
        // releases a parked participant and the existing runner reader on
        // success, propagated error, or panic. This helper creates no threads.
        drop(self.runtime.take());
        drop(self.runner.take());
    }
}

pub(super) fn run(options: &SessionOptions, runner: Runner) -> Result<(i32, Vec<u8>)> {
    if options.runtime != Path::new(BIN)
        || options.runtime_prefix.first().map(String::as_str) != Some("fake-lamp-voice")
        || !matches!(options.mode, SessionMode::Serve { .. })
    {
        return Err(invalid(
            "relay clock requires the original local serve fake",
        ));
    }
    let parent = options
        .work
        .parent()
        .filter(|path| path.is_absolute())
        .ok_or_else(|| invalid("relay clock needs an absolute private work parent"))?;
    let control_path = parent.join("serve-relay-clock.sock");
    let listener = ParentListener::bind(&control_path)?;
    let mut controlled = options.clone();
    controlled.runtime_prefix = vec![
        "fake-clock-serve-runtime".into(),
        "--fixture-control".into(),
        control_path.display().to_string(),
    ];
    controlled
        .runtime_prefix
        .extend_from_slice(&options.runtime_prefix[1..]);
    let (writer, input) = UnixStream::pair()?;
    let hard_due_us =
        EPOCH_US + (u64::from(options.seconds) + u64::from(options.allowance_seconds)) * 1_000_000;
    let clock = Cell::new(EPOCH_US);
    let control = RefCell::new(Control::new(listener, writer, runner, hard_due_us)?);
    let mut out = Vec::new();
    let result = session::run_with_controlled_clock(
        &controlled,
        input,
        &mut out,
        || MonoTime::from_micros(clock.get()),
        || control.borrow_mut().before(clock.get()),
        |poll| control.borrow_mut().completed(&clock, poll),
    );
    let ended = control.borrow().relay_ended;
    drop(control);
    let code = result?;
    if !ended {
        return Err(invalid("serve relay returned without actual terminal work"));
    }
    Ok((code, out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Read, panic::AssertUnwindSafe};

    fn fixture(runner: Runner) -> (crate::common::Private, Control, UnixStream) {
        let dir = crate::common::Private::new("relay-clock-boundary");
        let listener = ParentListener::bind(&dir.join("control.sock")).unwrap();
        let (writer, reader) = UnixStream::pair().unwrap();
        reader.set_nonblocking(true).unwrap();
        let control = Control::new(listener, writer, runner, EPOCH_US + 65_000_000).unwrap();
        (dir, control, reader)
    }

    /// The reader at EOF: every copy of the writer gone. A child another
    /// test forks at that moment holds a copy until its exec closes it
    /// (CLOEXEC), so EOF can come a little after the drop, and a single
    /// non-blocking read would see `WouldBlock` (Nine on #705, 2 of 10
    /// runs). So it reads until EOF, for at most 5 s. (A read timeout
    /// can't be set instead: macOS refuses it, `EINVAL`, on a socket whose
    /// peer has closed.)
    fn assert_eof(reader: &mut UnixStream) {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            match reader.read(&mut [0; 1]) {
                Ok(0) => return,
                Ok(_) => panic!("data where EOF was expected"),
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < until =>
                {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("no EOF within 5 s: {error}"),
            }
        }
    }

    fn poll(sequence: u64) -> RelayPollProgress {
        RelayPollProgress {
            poll_seq: sequence,
            ..Default::default()
        }
    }

    #[test]
    fn the_real_stop_keeps_the_full_delay_and_original_bytes_once() {
        let (_dir, mut control, mut reader) = fixture(Runner::StopAfter(Duration::from_secs(3)));
        let due = EPOCH_US + 3_000_000;
        let mut bytes = [0; 128];
        for at in [EPOCH_US, due - 1, due - 1] {
            control.fire_runner(at).unwrap();
            assert_eq!(
                reader.read(&mut bytes).unwrap_err().kind(),
                io::ErrorKind::WouldBlock
            );
            assert_eq!(control.runner_frames, 0);
        }
        control.fire_runner(due).unwrap();
        let count = reader.read(&mut bytes).unwrap();
        assert_eq!(&bytes[..count], STOP_LINE);
        assert_eq!(control.runner_frames, 1);
        control.fire_runner(due + POLL_US).unwrap();
        assert_eq!(
            reader.read(&mut bytes).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(control.runner_frames, 1);
    }

    #[test]
    fn real_runner_eof_holds_time_until_the_relay_reports_its_signal() {
        let (_dir, mut control, mut reader) = fixture(Runner::CloseAfter(Duration::from_secs(3)));
        let due = EPOCH_US + 3_000_000;
        control.fire_runner(due).unwrap();
        assert_eof(&mut reader);
        assert_eq!(control.runner_frames, 0);
        let clock = Cell::new(due);
        control.completed(&clock, poll(1)).unwrap();
        assert_eq!(clock.get(), due);
        assert!(!control.observe_signal);
        control
            .completed(
                &clock,
                RelayPollProgress {
                    stop_signal_sent_us: Some(due),
                    ..poll(2)
                },
            )
            .unwrap();
        assert_eq!(clock.get(), due);
        assert!(control.observe_signal);
        assert!(control.pending_stop_until.is_none());
    }

    #[test]
    fn runtime_completion_never_advances_time_or_claims_relay_exit() {
        let (_dir, mut control, _reader) = fixture(Runner::Silent);
        let clock = Cell::new(EPOCH_US);
        control.runtime_ended = true;
        control.terminal_until = Some(Instant::now() + HOST_GUARD);
        control.completed(&clock, poll(1)).unwrap();
        assert_eq!(clock.get(), EPOCH_US);
        assert!(!control.relay_ended);
        control
            .completed(
                &clock,
                RelayPollProgress {
                    terminal: true,
                    ..poll(2)
                },
            )
            .unwrap();
        assert_eq!(clock.get(), EPOCH_US);
        assert!(control.relay_ended);
    }

    #[test]
    fn an_expired_host_boundary_fails_before_another_step_or_clock_advance() {
        let (_dir, mut control, _reader) = fixture(Runner::Silent);
        control.terminal_until = Some(Instant::now());
        let error = control.before(EPOCH_US).unwrap_err();
        assert_eq!(
            error.downcast_ref::<io::Error>().unwrap().kind(),
            io::ErrorKind::TimedOut
        );
        assert!(control.runtime.is_none());
        assert_eq!(control.runtime_round, 0);
    }

    #[test]
    fn polls_preserve_two_milliseconds_and_stop_at_each_original_boundary() {
        let (_dir, mut control, _reader) = fixture(Runner::Silent);
        let clock = Cell::new(EPOCH_US);
        control.completed(&clock, poll(1)).unwrap();
        assert_eq!(clock.get(), EPOCH_US + 2_000);

        control.hard_due_us = EPOCH_US + 6_000_000;
        clock.set(control.hard_due_us - 1);
        control.completed(&clock, poll(2)).unwrap();
        assert_eq!(clock.get(), EPOCH_US + 6_000_000);

        control.signal_sent_us = Some(EPOCH_US + 3_000_000);
        clock.set(EPOCH_US + 13_000_000 - 1);
        control
            .completed(
                &clock,
                RelayPollProgress {
                    stop_signal_sent_us: control.signal_sent_us,
                    ..poll(3)
                },
            )
            .unwrap();
        assert_eq!(session::SERVE_STOP_GRACE_US, 10_000_000);
        assert_eq!(clock.get(), EPOCH_US + 13_000_000);
    }

    #[test]
    fn unwinding_releases_the_real_runner_reader_and_control_socket() {
        let (dir, control, mut reader) = fixture(Runner::Silent);
        let result = std::panic::catch_unwind(AssertUnwindSafe(move || {
            let _control = control;
            panic!("exercise owned endpoint cleanup");
        }));
        assert!(result.is_err());
        assert_eof(&mut reader);
        assert!(!dir.join("control.sock").exists());
    }
}
