//! Selected Controls privacy wait. Only observation and a narrow muted display
//! exist here. Audio starts only after both previous workers have exited.
use crate::{
    privacy::{PRIVACY_FRESH_US, PrivacyGate, PrivacyState},
    privacy_bus,
    process::{RingStartup, SessionDirectory, Worker, new_boot},
    ring_wire::{RingBlankReason, RingFeedback},
    service_privacy::{
        self, Result, WaitOutcome, finish, is_unavailable, unavailable, unavailable_io,
    },
    termination::Termination,
    wire::{Control, WorkerEvent},
};
use lamp_ipc::monotonic_us;
use lamp_turn_owner::{BootId, Controller, MonoTime, Permission};
use std::{
    fs, io,
    path::Path,
    time::{Duration, Instant},
};

/// How long the selected route waits before reading an unreadable switch
/// again, with a fresh worker.
const UNREADABLE_RETRY: Duration = Duration::from_secs(1);

pub(crate) fn wait_selected(
    executable: &Path,
    termination: &Termination,
    on_state: impl FnMut(PrivacyState, BootId, u64) -> Result<()>,
    mut additional_gate: impl FnMut() -> Result<bool>,
) -> Result<WaitOutcome> {
    let Some((root, route)) = privacy_bus::from_environment()? else {
        return service_privacy::wait_with_gate(executable, termination, on_state, additional_gate);
    };
    // Each retry says the switch reads unavailable, as a running worker's
    // wait does, so the health says the microphones are off until it can be
    // read, also while each fresh worker ends at its start (the first
    // builder on #2506). No worker has this session: it is the retry's.
    let on_state = std::cell::RefCell::new(on_state);
    let retry_session = new_boot()?;
    unreadable_retried(
        termination,
        UNREADABLE_RETRY,
        || {
            wait(
                executable,
                &root,
                route,
                termination,
                |state, boot, at_us| (*on_state.borrow_mut())(state, boot, at_us),
                &mut additional_gate,
            )
        },
        |at_us| (*on_state.borrow_mut())(PrivacyState::Unavailable, retry_session, at_us),
    )
}

/// A switch that cannot be read keeps the lamp closed and is read again,
/// each `retry`, by a fresh wait whose workers were cleaned up and verified;
/// it never ends the service (#795). Each new reason is said once.
pub(crate) fn unreadable_retried(
    termination: &Termination,
    retry: Duration,
    mut wait: impl FnMut() -> Result<WaitOutcome>,
    mut unreadable: impl FnMut(u64) -> Result<()>,
) -> Result<WaitOutcome> {
    let mut said: Option<String> = None;
    loop {
        match wait() {
            Err(error) if is_unavailable(&*error) => {
                unreadable(monotonic_us())?;
                let why = error.to_string();
                if said.as_deref() != Some(why.as_str()) {
                    eprintln!(
                        "lamp-voice: the microphones stay closed and the switch is read again each {} ms: {why}",
                        retry.as_millis()
                    );
                    said = Some(why);
                }
                let until = Instant::now() + retry;
                while Instant::now() < until {
                    if termination.signal_name().is_some() {
                        return Ok(WaitOutcome::Stopped);
                    }
                    std::thread::sleep(Duration::from_millis(20).min(retry));
                }
            }
            other => return other,
        }
    }
}

fn wait(
    executable: &Path,
    root: &Path,
    route: privacy_bus::Route,
    termination: &Termination,
    on_state: impl FnMut(PrivacyState, BootId, u64) -> Result<()>,
    mut additional_gate: impl FnMut() -> Result<bool>,
) -> Result<WaitOutcome> {
    // The durable software latch was restored by the caller before this point.
    // Unlike the legacy path, the selected slide is observed even during a
    // software mute. This creates neither capture workers nor permission.
    additional_gate()?;
    termination.check()?;
    let directory = SessionDirectory::create()?;
    let boot = new_boot()?;
    let args = privacy_bus::worker_arguments(Some((root, route)))?;
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let worker = started(Worker::spawn_with_tick(
        executable,
        &directory.path,
        "privacy",
        boot,
        &args,
        || termination.check(),
    ))?;
    let path = directory.path.clone();
    // Optional SPI startup is isolated while the mandatory privacy worker,
    // gate and controller continue on every bounded tick.
    let start_ring = |tick: &mut dyn FnMut() -> io::Result<()>| {
        let root_text = root
            .to_str()
            .ok_or_else(|| io::Error::other("privacy root must be UTF-8"))?;
        Worker::spawn_optional_ring_with_tick(
            executable,
            &path,
            boot,
            &["--privacy", "--bus-root", root_text],
            tick,
        )
    };
    run(
        worker,
        directory,
        boot,
        termination,
        on_state,
        additional_gate,
        indicator_started(route),
        start_ring,
    )
}

/// Whether the wait may start the muted indicator (`worker ring --privacy`),
/// which paints only a verified physical LOW. A lamp with no slide has no
/// LOW to paint, so its ring stays as with the route off, with one writer:
/// the asleep and tap faces, never this worker.
fn indicator_started(route: privacy_bus::Route) -> bool {
    route == privacy_bus::Route::Slide
}

/// The privacy worker as started: one that ended cleanly before its start
/// was read is an unreadable switch, read again after the retry like any
/// other. It ends itself cleanly when it cannot read the switch, and does so
/// at once while Controls restarts under it (a message from the swap it
/// must drop, or no fresh reading within 50 ms): on lamp-0c89's update
/// round that end, taken as a fault, ended the voice instead (#1396). A
/// worker that crashed (a panic, a signal, any other status) and any other
/// start failure end the service as before (the reviewer-lead's P2 on
/// #2506).
pub(crate) fn started(worker: io::Result<Worker>) -> Result<Worker> {
    worker.map_err(|error| {
        if Worker::ended_cleanly(&error) {
            unavailable_io(error).into()
        } else {
            error.into()
        }
    })
}

/// The muted wait itself, with the optional ring's start injected.
#[allow(clippy::too_many_arguments)]
fn run(
    mut worker: Worker,
    directory: SessionDirectory,
    boot: BootId,
    termination: &Termination,
    mut on_state: impl FnMut(PrivacyState, BootId, u64) -> Result<()>,
    mut additional_gate: impl FnMut() -> Result<bool>,
    indicator_allowed: bool,
    mut start_ring: impl FnMut(&mut dyn FnMut() -> io::Result<()>) -> io::Result<RingStartup>,
) -> Result<WaitOutcome> {
    let mut waiting = Waiting::new(boot)?;
    let mut indicator: Option<Box<Worker>> = None;
    let mut attempted = false;
    let mut indicator_failure = None;
    let mut indicator_failed_at = None;
    let mut indicator_kill_requested = false;
    let result = (|| {
        loop {
            if termination.signal_name().is_some() {
                return Ok(WaitOutcome::Stopped);
            }
            let state = waiting.step(
                &mut worker,
                if indicator_failure.is_none() {
                    indicator.as_deref_mut()
                } else {
                    None
                },
                &mut on_state,
                &mut additional_gate,
            )?;
            if state.allowed {
                return Ok(WaitOutcome::Allowed);
            }
            if indicator_allowed && state.physical == PrivacyState::Muted && !attempted {
                attempted = true;
                let started = start_ring(&mut || {
                    termination.check()?;
                    waiting
                        .step(&mut worker, None, &mut on_state, &mut additional_gate)
                        .map(|_| ())
                        .map_err(|error| {
                            if is_unavailable(&*error) {
                                unavailable_io(error)
                            } else {
                                io::Error::other(error.to_string())
                            }
                        })
                })?;
                match started {
                    RingStartup::Ready(child) => indicator = Some(child),
                    // No writer process exists, so no SPI was opened and no
                    // black write is owed: the wait goes on without its light.
                    // An optional light never ends a healthy voice.
                    RingStartup::Unavailable {
                        worker: None,
                        error,
                    } => {
                        eprintln!("lamp-voice: muted indication unavailable: {error}");
                    }
                    // A started writer keeps the verified black write and exit
                    // requirement before anything replaces it.
                    RingStartup::Unavailable {
                        worker: Some(mut child),
                        error,
                    } => {
                        eprintln!("lamp-voice: muted indication unavailable: {error}");
                        indicator_failure = Some(error);
                        indicator_failed_at = Some(monotonic_us());
                        let _ = child.channels.control.send(Control::Stop);
                        indicator = Some(child);
                    }
                }
            }
            if let Some(child) = &mut indicator {
                if indicator_failure.is_none() {
                    if let Err(error) = child.check_running() {
                        indicator_failure = Some(error);
                    } else {
                        for _ in 0..16 {
                            match child.channels.control.receive::<WorkerEvent>() {
                                Ok(Some(WorkerEvent::Ring {
                                    report: RingFeedback::Blanked { .. },
                                })) => {}
                                Ok(None) => break,
                                Ok(Some(event)) => {
                                    indicator_failure = Some(io::Error::other(format!(
                                        "unexpected privacy indicator event: {event:?}"
                                    )));
                                    break;
                                }
                                Err(error) => {
                                    indicator_failure = Some(error);
                                    break;
                                }
                            }
                        }
                    }
                    if let Some(error) = &indicator_failure {
                        indicator_failed_at = Some(monotonic_us());
                        eprintln!("lamp-voice: muted indication failed: {error}");
                        let _ = child.channels.control.send(Control::Stop);
                    }
                }
                if indicator_failure.is_some()
                    && !indicator_kill_requested
                    && indicator_failed_at
                        .is_some_and(|at| monotonic_us().saturating_sub(at) >= 300_000)
                    && !matches!(child.try_exit(), Ok(Some(_)))
                {
                    indicator_kill_requested = true;
                    if let Err(error) = child.request_termination() {
                        eprintln!("lamp-voice: privacy indicator termination failed: {error}");
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    })();
    let cleanup = cleanup(&mut worker, indicator.as_deref_mut());
    drop(indicator);
    drop(worker);
    let result = finish(result, cleanup);
    // Keep a failed writer's evidence distinct from permission to start a new
    // one, even when its process was subsequently reaped.
    let result = finish(result, indicator_failure.map_or(Ok(()), Err));
    finish(result, fs::remove_dir(&directory.path))
}

struct Step {
    physical: PrivacyState,
    allowed: bool,
}

struct Waiting {
    controller: Controller,
    boot: BootId,
    gate: PrivacyGate,
    sequence: u64,
    previous: Option<PrivacyState>,
    last_publish: Option<u64>,
    started: Instant,
}

impl Waiting {
    fn new(boot: BootId) -> Result<Self> {
        let mut controller = Controller::new(boot, MonoTime::from_micros(monotonic_us()));
        controller
            .set_microphone_permission(MonoTime::from_micros(monotonic_us()), Permission::Denied)?;
        controller
            .set_camera_permission(MonoTime::from_micros(monotonic_us()), Permission::Denied)?;
        Ok(Self {
            controller,
            boot,
            gate: PrivacyGate::default(),
            sequence: 0,
            previous: None,
            last_publish: None,
            started: Instant::now(),
        })
    }

    fn step(
        &mut self,
        worker: &mut Worker,
        indicator: Option<&mut Worker>,
        on_state: &mut impl FnMut(PrivacyState, BootId, u64) -> Result<()>,
        additional_gate: &mut impl FnMut() -> Result<bool>,
    ) -> Result<Step> {
        // Slow durable software-clear work happens while no audio exists. Age
        // the physical evidence again afterward; never reuse an old Allowed.
        let software_allowed = additional_gate()?;
        worker.check_running().map_err(unavailable)?;
        let at = monotonic_us();
        if self.sequence != 0
            && self.gate.state(MonoTime::from_micros(at)) == PrivacyState::Unavailable
        {
            return Err(unavailable("Controls privacy freshness lost while waiting"));
        }
        if self
            .last_publish
            .is_none_or(|last| at.saturating_sub(last) >= 25_000)
        {
            let snapshot = self.controller.snapshot(MonoTime::from_micros(at))?;
            worker
                .channels
                .control
                .send(Control::Authority { snapshot })
                .map_err(unavailable)?;
            if let Some(indicator) = indicator {
                let _ = indicator
                    .channels
                    .control
                    .send(Control::Authority { snapshot });
            }
            self.last_publish = Some(at);
        }
        let mut empty = false;
        for _ in 0..16 {
            match worker
                .channels
                .control
                .receive::<WorkerEvent>()
                .map_err(unavailable)?
            {
                Some(WorkerEvent::Privacy {
                    acquired_at_us,
                    muted,
                    ..
                }) => {
                    self.sequence = self
                        .sequence
                        .checked_add(1)
                        .ok_or_else(|| io::Error::other("privacy observation count exhausted"))?;
                    self.gate.observe(
                        self.sequence,
                        MonoTime::from_micros(acquired_at_us),
                        MonoTime::from_micros(monotonic_us()),
                        muted,
                    );
                    if self.gate.state(MonoTime::from_micros(monotonic_us()))
                        == PrivacyState::Unavailable
                    {
                        return Err(unavailable(
                            "invalid Controls privacy observation while waiting",
                        ));
                    }
                }
                Some(WorkerEvent::Fault { code }) => return Err(unavailable(code)),
                Some(_) => {
                    return Err(io::Error::other("unexpected Controls privacy wait event").into());
                }
                None => {
                    empty = true;
                    break;
                }
            }
        }
        let at = monotonic_us();
        let physical = self.gate.state(MonoTime::from_micros(at));
        if physical == PrivacyState::Unavailable
            && (self.sequence != 0
                || self.started.elapsed() >= Duration::from_micros(PRIVACY_FRESH_US))
        {
            return Err(unavailable("Controls privacy unavailable while waiting"));
        }
        if self.previous != Some(physical) {
            on_state(physical, self.boot, at)?;
            self.previous = Some(physical);
        }
        Ok(Step {
            physical,
            allowed: software_allowed && empty && physical == PrivacyState::Allowed,
        })
    }
}

/// Send Stop to both workers before waiting. Both share one 300 ms userspace
/// grace, followed by explicit forced cleanup on expiry (always a failure).
fn cleanup(privacy: &mut Worker, mut indicator: Option<&mut Worker>) -> io::Result<()> {
    let stopped_at = monotonic_us();
    let deadline = Instant::now() + Duration::from_millis(300);
    // A worker that ended on its own (an unreadable switch, #795) cannot
    // take a Stop: its exit status, read below within the same grace, is the
    // verification. It closes its socket before its exit can be read, so a
    // refused Stop is no failure by itself: lamp-52e6's cold boot, 27 Oct,
    // `privacy.c.p: Connection refused` with the publisher not yet up ended
    // the voice and left the restart guard's mute (#3697). A worker that
    // does not end within the grace is still forced and a failure.
    let mut refused: Option<io::Error> = None;
    let mut error = None;
    match privacy.channels.control.send(Control::Stop) {
        Ok(()) => {
            #[cfg(test)]
            privacy.run_on_stop();
        }
        Err(failure) => refused = Some(failure),
    }
    if let Some(child) = &mut indicator {
        match child.channels.control.send(Control::Stop) {
            Ok(()) => {
                #[cfg(test)]
                child.run_on_stop();
            }
            Err(failure) => {
                error.get_or_insert(failure);
            }
        }
    }
    let mut privacy_exited = false;
    let mut indicator_exited = indicator.is_none();
    let mut blanked = false;
    loop {
        if let Some(child) = &mut indicator {
            drain_shutdown(child, stopped_at, &mut blanked, &mut error);
            if !indicator_exited {
                match child.try_exit() {
                    Ok(Some(status)) => {
                        indicator_exited = true;
                        // A receipt can arrive between the earlier drain and
                        // exit observation. With the sender gone, drain once
                        // more before deciding whether a black write happened.
                        drain_shutdown(child, stopped_at, &mut blanked, &mut error);
                        if let Err(failure) = child.exit_result(status) {
                            error.get_or_insert(failure);
                        }
                    }
                    Ok(None) => {}
                    Err(failure) => {
                        error.get_or_insert(failure);
                    }
                }
            }
        }
        if !privacy_exited {
            match privacy.try_exit() {
                Ok(Some(status)) => {
                    privacy_exited = true;
                    if let Err(failure) = privacy.exit_result(status) {
                        error.get_or_insert(failure);
                    }
                }
                Ok(None) => {}
                Err(failure) => {
                    error.get_or_insert(failure);
                }
            }
        }
        if privacy_exited && indicator_exited {
            break;
        }
        if Instant::now() >= deadline {
            if !privacy_exited {
                let mut reason = "privacy wait shared stop deadline exceeded".to_owned();
                if let Some(refused) = &refused {
                    reason.push_str(&format!("; stop delivery failed: {refused}"));
                }
                let failure = privacy.terminate_and_reap(io::ErrorKind::TimedOut, reason);
                error.get_or_insert(failure);
            }
            if !indicator_exited && let Some(child) = &mut indicator {
                let failure = child.terminate_and_reap(
                    io::ErrorKind::TimedOut,
                    "privacy wait shared stop deadline exceeded".into(),
                );
                error.get_or_insert(failure);
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    if indicator.is_some() && !blanked {
        error
            .get_or_insert_with(|| io::Error::other("privacy indicator black write not confirmed"));
    }
    error.map_or(Ok(()), Err)
}

fn drain_shutdown(
    child: &mut Worker,
    stopped_at: u64,
    blanked: &mut bool,
    error: &mut Option<io::Error>,
) {
    for _ in 0..16 {
        match child.channels.control.receive::<WorkerEvent>() {
            Ok(Some(WorkerEvent::Ring {
                report:
                    report @ RingFeedback::Blanked {
                        reason: RingBlankReason::Shutdown,
                        ..
                    },
            })) => {
                if !*blanked && report.confirms_shutdown(stopped_at, monotonic_us()) {
                    *blanked = true;
                } else {
                    error.get_or_insert_with(|| {
                        io::Error::other("invalid or duplicate privacy indicator shutdown receipt")
                    });
                }
            }
            Ok(Some(WorkerEvent::Ring { .. })) => {}
            Ok(Some(_)) => {
                error.get_or_insert_with(|| {
                    io::Error::other("unexpected privacy indicator cleanup event")
                });
            }
            Ok(None) => break,
            Err(failure) => {
                error.get_or_insert(failure);
                break;
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::service_privacy::tests::{Script, mock};
    use std::sync::{Arc, atomic::AtomicBool};

    /// [`unreadable_retried`] with no report of the switch between reads.
    fn retried(
        termination: &Termination,
        retry: Duration,
        wait: impl FnMut() -> Result<WaitOutcome>,
    ) -> Result<WaitOutcome> {
        unreadable_retried(termination, retry, wait, |_| Ok(()))
    }

    /// lamp-4ace, 2026-10-16 10:04: a switch Controls could not read sent
    /// the voice into a crash loop. Now each wait that ends on it, with
    /// verified cleanup, is followed by another after the retry, and the
    /// first that is allowed or stopped is the outcome.
    #[test]
    fn an_unreadable_switch_is_read_again_and_never_ends_the_service() {
        let mut waits = 0;
        let outcome = retried(&Termination::inactive(), Duration::from_millis(1), || {
            waits += 1;
            if waits < 3 {
                Err(unavailable("physical privacy acquisition unavailable"))
            } else {
                Ok(WaitOutcome::Allowed)
            }
        })
        .unwrap();
        assert_eq!((outcome, waits), (WaitOutcome::Allowed, 3));
        // The worker's exit carried by an io::Error counts the same.
        let mut waits = 0;
        retried(&Termination::inactive(), Duration::from_millis(1), || {
            waits += 1;
            if waits == 1 {
                Err(unavailable_io("privacy worker exited: exit status: 0").into())
            } else {
                Ok(WaitOutcome::Stopped)
            }
        })
        .unwrap();
        assert_eq!(waits, 2);
    }

    /// A privacy worker's process that ended by itself with `command`'s
    /// status, as `check_running` finds it.
    #[track_caller]
    pub(crate) fn a_privacy_worker_ended(command: &str) -> io::Error {
        use std::os::unix::process::ExitStatusExt;

        let caller = std::panic::Location::caller();
        let fixture_started = Instant::now();
        let directory = SessionDirectory::create().unwrap();
        // Only this deliberate crash avoids an unnecessary regular core file.
        // Linux can ignore this limit for a piped core collector; this is not
        // evidence of what delayed any earlier fixture's exit observation.
        let child_command = if command == "kill -SEGV $$" {
            "ulimit -c 0 && kill -SEGV $$"
        } else {
            command
        };
        let (mut worker, _peer) =
            Worker::command_fixture(&directory.path, "privacy", child_command);
        let wait_started = Instant::now();
        let deadline = wait_started + Duration::from_secs(5);
        let pid = worker.process_id();
        // The fixture's R marker proves its first shell started, not that the
        // inner command ended. Only the observed child status proves the latter.
        eprintln!(
            "privacy exit fixture returned: command={command:?}, child_command={child_command:?}, caller={}:{}, pid={pid}, setup_us={}",
            caller.file(),
            caller.line(),
            wait_started.duration_since(fixture_started).as_micros()
        );
        let mut polls = 0_u64;
        let status = loop {
            polls += 1;
            match worker.try_exit() {
                Ok(Some(status)) => break status,
                Ok(None) => assert!(
                    Instant::now() < deadline,
                    "the stand-in did not end: command={command:?}, caller={}:{}, pid={pid}, polls={polls}, wait_us={}, total_us={}, last_try_exit=None",
                    caller.file(),
                    caller.line(),
                    wait_started.elapsed().as_micros(),
                    fixture_started.elapsed().as_micros()
                ),
                Err(error) => panic!(
                    "stand-in exit observation failed: command={command:?}, caller={}:{}, pid={pid}, polls={polls}, wait_us={}, kind={:?}, raw_os_error={:?}: {error}",
                    caller.file(),
                    caller.line(),
                    wait_started.elapsed().as_micros(),
                    error.kind(),
                    error.raw_os_error()
                ),
            }
            std::thread::sleep(Duration::from_millis(2));
        };
        eprintln!(
            "privacy exit fixture observed: command={command:?}, caller={}:{}, pid={pid}, polls={polls}, wait_us={}, total_us={}, status={status}, code={:?}, signal={:?}, core_dumped={}",
            caller.file(),
            caller.line(),
            wait_started.elapsed().as_micros(),
            fixture_started.elapsed().as_micros(),
            status.code(),
            status.signal(),
            status.core_dumped()
        );
        if command == "kill -SEGV $$" {
            assert_eq!(
                status.signal(),
                Some(rustix::process::Signal::SEGV.as_raw()),
                "the stand-in must really end by SIGSEGV: pid={pid}, status={status}"
            );
        } else if command == "exit 101" {
            assert_eq!(
                status.code(),
                Some(101),
                "the stand-in must really exit 101: pid={pid}, status={status}"
            );
        }
        let ended = worker.check_running().unwrap_err();
        drop(worker);
        let _ = fs::remove_dir(&directory.path);
        ended
    }

    /// lamp-0c89, 21 Oct 13:13 (#1396): a privacy worker that ended cleanly
    /// before its start was read, as one does while Controls restarts under
    /// it, is an unreadable switch, read again after the retry, and each
    /// retry says so to the wait. One that crashed (a panic's 101, a signal)
    /// and another start failure end the service as before (the
    /// reviewer-lead's P2 on #2506).
    #[test]
    fn only_a_privacy_worker_that_ended_cleanly_at_its_start_is_an_unreadable_switch() {
        let clean = a_privacy_worker_ended("exit 0");
        assert!(Worker::ended_cleanly(&clean), "{clean}");
        assert!(
            clean.to_string().starts_with("privacy worker exited"),
            "{clean}"
        );
        let mut clean = Some(clean);
        let (mut waits, mut said) = (0, Vec::new());
        let outcome = unreadable_retried(
            &Termination::inactive(),
            Duration::from_millis(1),
            || {
                waits += 1;
                match clean.take() {
                    Some(error) => started(Err(error)).map(|_| WaitOutcome::Allowed),
                    None => Ok(WaitOutcome::Allowed),
                }
            },
            |at_us| {
                said.push(at_us);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!((outcome, waits, said.len()), (WaitOutcome::Allowed, 2, 1));
        for command in ["exit 101", "kill -SEGV $$"] {
            let crashed = a_privacy_worker_ended(command);
            assert!(!Worker::ended_cleanly(&crashed), "{command}: {crashed}");
            let error = started(Err(crashed)).err().unwrap();
            assert!(!is_unavailable(&*error), "{command}: {error}");
            let mut waits = 0;
            let mut crashed = Some(a_privacy_worker_ended(command));
            let error = retried(&Termination::inactive(), Duration::from_millis(1), || {
                waits += 1;
                started(Err(crashed.take().unwrap())).map(|_| WaitOutcome::Allowed)
            })
            .unwrap_err();
            assert_eq!(waits, 1, "{command}: ends the service, not read again");
            assert!(!is_unavailable(&*error), "{command}: {error}");
        }
        let timed_out = started(Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "worker readiness timed out",
        )))
        .err()
        .unwrap();
        assert!(!is_unavailable(&*timed_out), "{timed_out}");
    }

    /// A worker that ended cleanly on its own, saying the switch was
    /// unreadable, has gone before its Stop: that is verified cleanup, so
    /// the wait can be tried again.
    #[test]
    fn a_worker_that_ended_cleanly_before_its_stop_is_verified_cleanup() {
        let directory = SessionDirectory::create().unwrap();
        let (mut worker, peer, input) = Worker::cooperative_fixture(&directory.path, "privacy");
        drop(peer);
        drop(input);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !matches!(worker.try_exit(), Ok(Some(_))) {
            assert!(Instant::now() < deadline, "the fixture did not end");
            std::thread::sleep(Duration::from_millis(2));
        }
        worker.send_stop_until_peer_gone();
        cleanup(&mut worker, None).unwrap();
        drop(worker);
        let _ = fs::remove_dir(&directory.path);
    }

    /// lamp-52e6's cold boot, 27 Oct (#3697): the voice started before
    /// Controls' privacy publisher was up. Each worker found no publisher,
    /// closed its socket and ended cleanly, and its Stop was refused before
    /// its exit could be read: that cleanup, taken as failed, ended the
    /// voice, and the restart guard muted the lamp. Now the switch stays
    /// unreadable, the microphones closed, and it is read again until the
    /// publisher is up and reads open.
    #[test]
    fn a_publisher_not_up_yet_keeps_the_microphones_closed_until_it_reads_open() {
        let states = std::cell::RefCell::new(Vec::new());
        let mut refused = 0;
        let outcome = unreadable_retried(
            &Termination::inactive(),
            Duration::from_millis(1),
            || {
                let fixtures = SessionDirectory::create().unwrap();
                let directory = SessionDirectory::create().unwrap();
                if refused < 3 {
                    refused += 1;
                    // The worker that found no publisher: its socket is
                    // closed, its process ends a moment later.
                    let (mut worker, peer, input) =
                        Worker::cooperative_fixture(&fixtures.path, "privacy");
                    drop(peer);
                    let late = std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(50));
                        drop(input);
                    });
                    let ended = finish(
                        Err(unavailable("privacy publisher unavailable")),
                        cleanup(&mut worker, None),
                    );
                    late.join().unwrap();
                    drop(worker);
                    let _ = fs::remove_dir(&directory.path);
                    return ended;
                }
                // The publisher is up and the switch reads open.
                let (worker, peer) = mock(
                    &fixtures.path,
                    Script::Unmute,
                    Arc::new(AtomicBool::new(false)),
                );
                let outcome = run(
                    worker,
                    directory,
                    new_boot().unwrap(),
                    &Termination::inactive(),
                    |state, _, _| {
                        states.borrow_mut().push(state);
                        Ok(())
                    },
                    || Ok(true),
                    false,
                    |_| unreachable!("no indicator on this wait"),
                );
                assert!(peer.join().unwrap().received_stop);
                outcome
            },
            |_| {
                states.borrow_mut().push(PrivacyState::Unavailable);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!((outcome, refused), (WaitOutcome::Allowed, 3));
        let states = states.into_inner();
        assert_eq!(states[..3], [PrivacyState::Unavailable; 3]);
        assert_eq!(states.last(), Some(&PrivacyState::Allowed));
    }

    #[test]
    fn a_stop_while_it_waits_to_read_again_ends_the_wait_and_other_errors_still_end_it() {
        let outcome = retried(&Termination::signaled(), Duration::from_secs(60), || {
            Err(unavailable("no switch"))
        })
        .unwrap();
        assert_eq!(outcome, WaitOutcome::Stopped);
        // A failed cleanup hides the unreadable switch: it ends the service.
        let mut waits = 0;
        let error = retried(&Termination::inactive(), Duration::from_millis(1), || {
            waits += 1;
            finish(
                Err::<WaitOutcome, _>(unavailable("no switch")),
                Err(io::Error::other("worker still open")),
            )
        })
        .unwrap_err();
        assert_eq!(waits, 1);
        assert!(
            error.to_string().contains("verified cleanup failed"),
            "{error}"
        );
        // So does any other error.
        let error = retried(&Termination::inactive(), Duration::from_millis(1), || {
            Err(io::Error::other("software mute store failed").into())
        })
        .unwrap_err();
        assert!(!is_unavailable(&*error));
    }

    #[test]
    fn selected_wait_observes_while_software_muted_without_granting() {
        let directory = SessionDirectory::create().unwrap();
        let (mut worker, peer) = mock(
            &directory.path,
            Script::Unmute,
            Arc::new(AtomicBool::new(false)),
        );
        let mut waiting = Waiting::new(new_boot().unwrap()).unwrap();
        let mut states = Vec::new();
        let mut record = |state, _, _| {
            states.push(state);
            Ok(())
        };
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let step = waiting
                .step(&mut worker, None, &mut record, &mut || Ok(false))
                .unwrap();
            assert!(!step.allowed, "physical open cleared the software latch");
            if step.physical == PrivacyState::Allowed {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "fixture did not produce stable open"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(
            waiting
                .step(&mut worker, None, &mut record, &mut || Ok(true))
                .unwrap()
                .allowed
        );
        cleanup(&mut worker, None).unwrap();
        let peer = peer.join().unwrap();
        assert!(peer.received_stop);
        assert!(peer.denied_heartbeats > 0);
        assert!(states.contains(&PrivacyState::Muted));
        assert!(states.contains(&PrivacyState::Allowed));
    }

    #[test]
    fn selected_wait_never_substitutes_software_clear_for_missing_or_bad_switch() {
        for script in [
            Script::Missing,
            Script::Stale,
            Script::Fault,
            Script::Invalid,
        ] {
            let directory = SessionDirectory::create().unwrap();
            let (mut worker, peer) =
                mock(&directory.path, script, Arc::new(AtomicBool::new(false)));
            let mut waiting = Waiting::new(new_boot().unwrap()).unwrap();
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                match waiting.step(&mut worker, None, &mut |_, _, _| Ok(()), &mut || Ok(true)) {
                    Err(_) => break,
                    Ok(step) => assert!(!step.allowed),
                }
                assert!(
                    Instant::now() < deadline,
                    "invalid input did not close wait"
                );
                std::thread::sleep(Duration::from_millis(2));
            }
            cleanup(&mut worker, None).unwrap();
            assert!(peer.join().unwrap().received_stop);
        }
    }

    fn stopping_child(
        directory: &Path,
        role: &str,
        receipts: usize,
    ) -> (
        Worker,
        std::thread::JoinHandle<crate::transport::WorkerChannels>,
    ) {
        let (mut worker, mut peer, input) = Worker::cooperative_fixture(directory, role);
        // The cleanup's Stop is taken on its own thread, right after it is
        // sent: the receipts go, the child's input closes, and its exit is
        // waited for there, as `service_privacy`'s mock does, so a busy host
        // cannot hold them past the cleanup's 300 ms (#1249; the reviewer's
        // #1551 gate saw the exit itself late). This thread only hands the
        // peer back once that has run.
        let (ended, end) = std::sync::mpsc::channel();
        let pid = worker.fixture_pid();
        worker.on_stop(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                match peer.control.receive::<Control>() {
                    Ok(Some(Control::Stop)) => break,
                    Ok(_) => {}
                    Err(error) => return Err(error),
                }
                if Instant::now() >= deadline {
                    return Err(io::Error::other("cleanup never requested Stop"));
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            for _ in 0..receipts {
                let now = monotonic_us();
                peer.control.send(WorkerEvent::Ring {
                    report: RingFeedback::Blanked {
                        reason: RingBlankReason::Shutdown,
                        write_started_at_us: now,
                        write_finished_at_us: now,
                    },
                })?;
            }
            drop(input);
            crate::scenario_clock::wait_child_exited(pid, Instant::now() + Duration::from_secs(5))?;
            let _ = ended.send(peer);
            Ok(())
        });
        let task = std::thread::spawn(move || {
            end.recv_timeout(Duration::from_secs(5))
                .expect("cleanup never requested Stop")
        });
        (worker, task)
    }

    /// Reviewer, #795: a ring that never started must not end the voice.
    #[test]
    fn a_ring_that_never_started_leaves_the_wait_to_open_without_its_light() {
        // The fixtures keep their endpoints until joined, so they live apart
        // from the session directory the wait removes when it ends.
        let fixtures = SessionDirectory::create().unwrap();
        let directory = SessionDirectory::create().unwrap();
        let (worker, peer) = mock(
            &fixtures.path,
            Script::Unmute,
            Arc::new(AtomicBool::new(false)),
        );
        let mut starts = 0;
        let outcome = run(
            worker,
            directory,
            new_boot().unwrap(),
            &Termination::inactive(),
            |_, _, _| Ok(()),
            || Ok(true),
            true,
            |tick| {
                starts += 1;
                tick()?;
                Ok(RingStartup::Unavailable {
                    worker: None,
                    error: io::Error::other("synthetic bind failure before spawn"),
                })
            },
        );
        assert!(matches!(outcome, Ok(WaitOutcome::Allowed)), "{outcome:?}");
        assert_eq!(starts, 1, "one attempt per muted wait");
        assert!(peer.join().unwrap().received_stop);
    }

    /// A lamp with no slide (`no-slide`) has no LOW for the muted indicator
    /// to paint, so its wait never starts that worker, even through a muted
    /// observation, and the ring keeps one writer: the asleep and tap faces,
    /// as with the route off.
    #[test]
    fn with_no_slide_the_wait_never_starts_the_muted_indicator() {
        assert!(indicator_started(privacy_bus::Route::Slide));
        assert!(!indicator_started(privacy_bus::Route::NoSlide));
        let fixtures = SessionDirectory::create().unwrap();
        let directory = SessionDirectory::create().unwrap();
        let muted_seen = Arc::new(AtomicBool::new(false));
        let (worker, peer) = mock(&fixtures.path, Script::Unmute, muted_seen.clone());
        let mut starts = 0;
        let outcome = run(
            worker,
            directory,
            new_boot().unwrap(),
            &Termination::inactive(),
            |_, _, _| Ok(()),
            || Ok(true),
            indicator_started(privacy_bus::Route::NoSlide),
            |_| {
                starts += 1;
                Ok(RingStartup::Unavailable {
                    worker: None,
                    error: io::Error::other("never started"),
                })
            },
        );
        assert!(matches!(outcome, Ok(WaitOutcome::Allowed)), "{outcome:?}");
        assert!(muted_seen.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(starts, 0);
        assert!(peer.join().unwrap().received_stop);
    }

    #[test]
    fn a_started_ring_that_failed_still_needs_its_black_write_and_exit() {
        for receipts in [0, 1] {
            let fixtures = SessionDirectory::create().unwrap();
            let directory = SessionDirectory::create().unwrap();
            let (worker, peer) = mock(
                &fixtures.path,
                Script::Unmute,
                Arc::new(AtomicBool::new(false)),
            );
            let (ring, ring_peer) = stopping_child(&fixtures.path, "ring", receipts);
            let mut ring = Some(Box::new(ring));
            let outcome = run(
                worker,
                directory,
                new_boot().unwrap(),
                &Termination::inactive(),
                |_, _, _| Ok(()),
                || Ok(true),
                true,
                |_| {
                    Ok(RingStartup::Unavailable {
                        worker: ring.take(),
                        error: io::Error::other("synthetic readiness failure after spawn"),
                    })
                },
            );
            assert!(outcome.is_err(), "receipts={receipts}: {outcome:?}");
            assert!(peer.join().unwrap().received_stop);
            drop(ring_peer.join().unwrap());
        }
    }

    #[test]
    fn next_writer_needs_one_black_receipt_and_both_verified_exits() {
        for receipts in [0, 1, 2] {
            let directory = SessionDirectory::create().unwrap();
            let (mut privacy, privacy_peer) = stopping_child(&directory.path, "privacy", 0);
            let (mut indicator, ring_peer) = stopping_child(&directory.path, "ring", receipts);
            let result = cleanup(&mut privacy, Some(&mut indicator));
            assert_eq!(
                result.is_ok(),
                receipts == 1,
                "receipts={receipts}: {result:?}"
            );
            assert!(privacy.try_exit().unwrap().unwrap().success());
            assert!(indicator.try_exit().unwrap().unwrap().success());
            drop(privacy_peer.join().unwrap());
            drop(ring_peer.join().unwrap());
        }
    }

    #[test]
    fn receipt_after_sender_exit_is_still_read_and_duplicate_is_rejected() {
        let directory = SessionDirectory::create().unwrap();
        let (mut child, mut peer, input) = Worker::cooperative_fixture(&directory.path, "ring");
        let stopped = monotonic_us();
        let at = monotonic_us();
        let receipt = || WorkerEvent::Ring {
            report: RingFeedback::Blanked {
                reason: RingBlankReason::Shutdown,
                write_started_at_us: at,
                write_finished_at_us: at,
            },
        };
        peer.control.send(receipt()).unwrap();
        drop(input);
        let mut blanked = false;
        let mut error = None;
        drain_shutdown(&mut child, stopped, &mut blanked, &mut error);
        assert!(blanked);
        assert!(error.is_none());
        peer.control.send(receipt()).unwrap();
        drain_shutdown(&mut child, stopped, &mut blanked, &mut error);
        assert!(error.is_some());
        child.shutdown().unwrap();
    }
}
