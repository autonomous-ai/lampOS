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
    fmt, fs, io,
    path::Path,
    time::{Duration, Instant},
};

/// How long the selected route waits before reading an unreadable switch
/// again, with a fresh worker.
const UNREADABLE_RETRY: Duration = Duration::from_secs(1);

/// How often a look of the wait sends the privacy worker its authority, as
/// it always has, after its gate.
const PUBLISH_EVERY_US: u64 = 25_000;

/// How often the authority is renewed before the gate and between the
/// gate's pieces of work ([`Waiting::step`], #4332): well inside the
/// worker's 250 ms lease, and never at a fast look, which the publish after
/// its gate already renewed.
const KEEP_EVERY_US: u64 = 50_000;

/// The wait's gate: whether the software latch allows, given `keep`, which
/// renews the privacy worker's Denied authority between the gate's pieces
/// of work (#4332), each call named by the check just done, so a refused
/// renewal says which. A gate with no worker yet is given one that does
/// nothing.
pub(crate) type Keep<'a> = &'a mut dyn FnMut(&'static str) -> Result<()>;

/// Which of the wait's renewals of the privacy worker's authority this is,
/// said when its send is refused (#4332): each fails closed the same way,
/// and the journal names the one that did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Renewal {
    /// The first, as soon as the worker's Ready was read.
    AfterReady,
    /// A look's, before its gate.
    BeforeGate,
    /// Inside a look's gate, after the named check.
    InGate(&'static str),
    /// A look's, after its gate.
    AfterGate,
}

impl fmt::Display for Renewal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AfterReady => f.write_str("renewal after Ready"),
            Self::BeforeGate => f.write_str("renewal before the gate"),
            Self::InGate(check) => write!(f, "renewal in the gate after {check}"),
            Self::AfterGate => f.write_str("renewal after the gate"),
        }
    }
}

pub(crate) fn wait_selected(
    executable: &Path,
    termination: &Termination,
    on_state: impl FnMut(PrivacyState, BootId, u64) -> Result<()>,
    mut additional_gate: impl FnMut(Keep<'_>) -> Result<bool>,
) -> Result<WaitOutcome> {
    let Some((root, route)) = privacy_bus::from_environment()? else {
        return service_privacy::wait_with_gate(executable, termination, on_state, move || {
            additional_gate(&mut |_| Ok(()))
        });
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
    mut additional_gate: impl FnMut(Keep<'_>) -> Result<bool>,
) -> Result<WaitOutcome> {
    // The durable software latch was restored by the caller before this point.
    // Unlike the legacy path, the selected slide is observed even during a
    // software mute. This creates neither capture workers nor permission.
    // No worker exists yet, so no lease to keep.
    additional_gate(&mut |_| Ok(()))?;
    termination.check()?;
    let directory = SessionDirectory::create()?;
    let boot = new_boot()?;
    let args = privacy_bus::worker_arguments(Some((root, route)))?;
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    // The wait's controller exists before its worker does, so nothing of
    // its own making comes between the worker's Ready and its first
    // authority (#4332).
    let mut waiting = Waiting::new(boot)?;
    let mut worker = started(Worker::spawn_with_tick(
        executable,
        &directory.path,
        "privacy",
        boot,
        &args,
        || termination.check(),
    ))?;
    // The worker's 250 ms lease runs from its start. Its first authority
    // goes at once, not at the first look: on lamp-06c1's cold boot with
    // the gate's renewals in, the lease still lapsed 250 ms after Ready,
    // before the wait's first line, and the voice exited 1 (#4332). A
    // refused send fails closed, as every renewal does, with the worker
    // stopped and its directory removed as after any failed look.
    let first = waiting.ready(&mut worker);
    if let Err(error) = first {
        let cleanup = cleanup(&mut worker, None);
        drop(worker);
        let result = finish(Err(error), cleanup);
        return finish(result, fs::remove_dir(&directory.path));
    }
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
        waiting,
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
    mut waiting: Waiting,
    termination: &Termination,
    mut on_state: impl FnMut(PrivacyState, BootId, u64) -> Result<()>,
    mut additional_gate: impl FnMut(Keep<'_>) -> Result<bool>,
    indicator_allowed: bool,
    mut start_ring: impl FnMut(&mut dyn FnMut() -> io::Result<()>) -> io::Result<RingStartup>,
) -> Result<WaitOutcome> {
    let mut indicator: Option<Box<Worker>> = None;
    let mut attempted = false;
    let mut indicator_failure = None;
    let mut indicator_failed_at = None;
    let mut indicator_kill_requested = false;
    // With LAMP_STALLS=on, a look of the wait or a gap between looks of
    // 50 ms or more is a line in `stalls-wait.log`, and each of the gate's
    // own checks that long a line in its `stalls-wait-<check>.log` (#4332).
    let mut stalls = crate::stalls::Stalls::from_environment("wait");
    let result = (|| {
        loop {
            if termination.signal_name().is_some() {
                return Ok(WaitOutcome::Stopped);
            }
            let pass = stalls.begin();
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
            pass.done();
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

    /// The worker's first authority, sent as soon as its Ready was read:
    /// none sent before, so no rate holds it back. The wait's own clock for
    /// a first reading starts here, as it did at the first look's start.
    fn ready(&mut self, worker: &mut Worker) -> Result<()> {
        self.started = Instant::now();
        self.renew(worker, None, 0, Renewal::AfterReady)
    }

    fn step(
        &mut self,
        worker: &mut Worker,
        mut indicator: Option<&mut Worker>,
        on_state: &mut impl FnMut(PrivacyState, BootId, u64) -> Result<()>,
        additional_gate: &mut impl FnMut(Keep<'_>) -> Result<bool>,
    ) -> Result<Step> {
        // The worker's lease is renewed before the gate and between its
        // pieces of work too, not only after it (#4332): on lamp-06c1's cold
        // boot the first look's gate held past 250 ms before any authority
        // reached the new worker, which ended on `privacy controller lease
        // expired`, and the voice with it. This authority is always the
        // wait's Denied one, so a renewal grants nothing; the lease keeps its
        // length and its expiry, and one piece of work held past it still
        // lapses it, closed.
        self.renew(
            worker,
            indicator.as_deref_mut(),
            KEEP_EVERY_US,
            Renewal::BeforeGate,
        )?;
        // Slow durable software-clear work happens while no audio exists. Age
        // the physical evidence again afterward; never reuse an old Allowed.
        let software_allowed = additional_gate(&mut |check| {
            self.renew(
                worker,
                indicator.as_deref_mut(),
                KEEP_EVERY_US,
                Renewal::InGate(check),
            )
        })?;
        worker.check_running().map_err(unavailable)?;
        let at = monotonic_us();
        if self.sequence != 0
            && self.gate.state(MonoTime::from_micros(at)) == PrivacyState::Unavailable
        {
            return Err(unavailable("Controls privacy freshness lost while waiting"));
        }
        self.renew(worker, indicator, PUBLISH_EVERY_US, Renewal::AfterGate)?;
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

    /// Sends the controller's snapshot, freshly timed, to the worker (and
    /// the indicator) when `every_us` or more have passed since the last
    /// one. The controller's permissions stay Denied throughout the wait.
    /// A refused send is the switch unavailable, named by `which`.
    fn renew(
        &mut self,
        worker: &mut Worker,
        indicator: Option<&mut Worker>,
        every_us: u64,
        which: Renewal,
    ) -> Result<()> {
        let at = monotonic_us();
        if self
            .last_publish
            .is_some_and(|last| at.saturating_sub(last) < every_us)
        {
            return Ok(());
        }
        let snapshot = self.controller.snapshot(MonoTime::from_micros(at))?;
        worker
            .channels
            .control
            .send(Control::Authority { snapshot })
            .map_err(|error| unavailable(format!("{error} ({which})")))?;
        if let Some(indicator) = indicator {
            let _ = indicator
                .channels
                .control
                .send(Control::Authority { snapshot });
        }
        self.last_publish = Some(at);
        Ok(())
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
                    Waiting::new(new_boot().unwrap()).unwrap(),
                    &Termination::inactive(),
                    |state, _, _| {
                        states.borrow_mut().push(state);
                        Ok(())
                    },
                    |_| Ok(true),
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
                .step(&mut worker, None, &mut record, &mut |_| Ok(false))
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
                .step(&mut worker, None, &mut record, &mut |_| Ok(true))
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
                match waiting.step(&mut worker, None, &mut |_, _, _| Ok(()), &mut |_| Ok(true)) {
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
            Waiting::new(new_boot().unwrap()).unwrap(),
            &Termination::inactive(),
            |_, _, _| Ok(()),
            |_| Ok(true),
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
            Waiting::new(new_boot().unwrap()).unwrap(),
            &Termination::inactive(),
            |_, _, _| Ok(()),
            |_| Ok(true),
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
                Waiting::new(new_boot().unwrap()).unwrap(),
                &Termination::inactive(),
                |_, _, _| Ok(()),
                |_| Ok(true),
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

    /// What a privacy worker stand-in saw of its authority: the longest
    /// stretch with none arriving, from its start, and whether its lease
    /// lapsed, by `privacy_bus::serve`'s own rule.
    struct Leased {
        longest_us: u64,
        lapsed: bool,
        authorities: usize,
    }

    /// A privacy worker that ends itself as the real one does when its
    /// controller's lease lapses (`privacy controller lease expired`): no
    /// authority within 250 ms of its start, or the one it holds expired.
    /// Every authority must be the wait's Denied one. It sends nothing.
    fn lease_watching(directory: &Path) -> (Worker, std::thread::JoinHandle<Leased>) {
        let (worker, mut peer, input) = Worker::cooperative_fixture(directory, "privacy");
        let task = std::thread::spawn(move || {
            let began = monotonic_us();
            let mut last = began;
            let mut expires: Option<u64> = None;
            let mut seen = Leased {
                longest_us: 0,
                lapsed: false,
                authorities: 0,
            };
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let now = monotonic_us();
                if expires.map_or(now.saturating_sub(began) >= 250_000, |at| at <= now) {
                    seen.lapsed = true;
                    seen.longest_us = seen.longest_us.max(now.saturating_sub(last));
                    drop(input);
                    drop(peer);
                    return seen;
                }
                match peer.control.receive::<Control>() {
                    Ok(Some(Control::Authority { snapshot })) => {
                        assert_eq!(snapshot.microphone_permission(), Permission::Denied);
                        assert_eq!(snapshot.camera_permission(), Permission::Denied);
                        let now = monotonic_us();
                        seen.longest_us = seen.longest_us.max(now.saturating_sub(last));
                        seen.authorities += 1;
                        last = now;
                        expires = Some(snapshot.expires_at().as_micros());
                        continue;
                    }
                    Ok(Some(Control::Stop)) | Err(_) => {
                        let now = monotonic_us();
                        seen.longest_us = seen.longest_us.max(now.saturating_sub(last));
                        drop(input);
                        return seen;
                    }
                    Ok(Some(other)) => panic!("not a privacy wait's command: {other:?}"),
                    Ok(None) => {}
                }
                assert!(
                    Instant::now() < deadline,
                    "the wait never stopped the stand-in"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
        });
        (worker, task)
    }

    /// lamp-06c1's cold boot, 30 Oct, run 1 (#4332): the wait's first look
    /// at the reminders, the controls and the health took past 250 ms
    /// before any authority reached the new privacy worker; its lease
    /// lapsed, it ended (`privacy controller lease expired`) and the voice
    /// with it. A gate of ~400 ms in ~100 ms steps, renewing between them,
    /// now keeps the worker's Denied authority arriving within 250 ms
    /// throughout.
    #[test]
    fn a_slow_gate_in_bounded_steps_keeps_the_privacy_workers_lease() {
        let directory = SessionDirectory::create().unwrap();
        let (mut worker, peer) = lease_watching(&directory.path);
        let mut waiting = Waiting::new(new_boot().unwrap()).unwrap();
        let mut gates = 0;
        for _ in 0..2 {
            let _ = waiting.step(&mut worker, None, &mut |_, _, _| Ok(()), &mut |keep| {
                gates += 1;
                for _ in 0..4 {
                    std::thread::sleep(Duration::from_millis(100));
                    keep("a check")?;
                }
                Ok(false)
            });
        }
        assert_eq!(gates, 2);
        let _ = worker.channels.control.send(Control::Stop);
        let seen = peer.join().unwrap();
        let _ = cleanup(&mut worker, None);
        drop(worker);
        let _ = fs::remove_dir(&directory.path);
        assert!(
            !seen.lapsed,
            "the lease lapsed after {} us",
            seen.longest_us
        );
        assert!(
            seen.longest_us < 250_000,
            "{} us with no authority",
            seen.longest_us
        );
        assert!(seen.authorities >= 8, "{} authorities", seen.authorities);
    }

    /// The renewals bound only the work between them: one call of the gate
    /// held past the lease still lapses it, and the wait fails closed, as
    /// a hang should (#4332; nothing hides it).
    #[test]
    fn one_gate_call_past_the_lease_still_lapses_it_and_fails_closed() {
        let directory = SessionDirectory::create().unwrap();
        let (mut worker, peer) = lease_watching(&directory.path);
        let mut waiting = Waiting::new(new_boot().unwrap()).unwrap();
        let step = waiting.step(&mut worker, None, &mut |_, _, _| Ok(()), &mut |_keep| {
            std::thread::sleep(Duration::from_millis(320));
            Ok(true)
        });
        let seen = peer.join().unwrap();
        // The stand-in has gone, so the next look cannot reach it.
        let after = waiting.step(&mut worker, None, &mut |_, _, _| Ok(()), &mut |_| Ok(true));
        let _ = cleanup(&mut worker, None);
        drop(worker);
        let _ = fs::remove_dir(&directory.path);
        assert!(seen.lapsed, "a 320 ms call kept the lease");
        assert!(!matches!(step, Ok(Step { allowed: true, .. })));
        assert!(after.is_err(), "the wait went on past a lapsed worker");
    }

    /// lamp-06c1's cold boot, 30 Oct, run 2, with the gate's renewals in
    /// (#4332): the lease still lapsed 250 ms after the worker's Ready,
    /// before the wait's first line. The worker's 250 ms runs from its own
    /// start, before the wait reads its Ready (~100 ms here, as under a
    /// cold boot's load); ~200 ms more then pass before the first look,
    /// whose gate takes ~100 ms of its own. The authority sent at Ready
    /// keeps the lease; with none until the first look it lapsed.
    #[test]
    fn the_first_authority_goes_at_ready_not_at_the_first_look() {
        let directory = SessionDirectory::create().unwrap();
        let mut waiting = Waiting::new(new_boot().unwrap()).unwrap();
        let (mut worker, peer) = lease_watching(&directory.path);
        std::thread::sleep(Duration::from_millis(100));
        waiting.ready(&mut worker).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        let _ = waiting.step(&mut worker, None, &mut |_, _, _| Ok(()), &mut |keep| {
            std::thread::sleep(Duration::from_millis(50));
            keep("a check")?;
            std::thread::sleep(Duration::from_millis(50));
            Ok(false)
        });
        let _ = worker.channels.control.send(Control::Stop);
        let seen = peer.join().unwrap();
        let _ = cleanup(&mut worker, None);
        drop(worker);
        let _ = fs::remove_dir(&directory.path);
        assert!(
            !seen.lapsed,
            "the lease lapsed after {} us",
            seen.longest_us
        );
        assert!(
            seen.longest_us < 250_000,
            "{} us with no authority",
            seen.longest_us
        );
    }

    /// Whichever renewal finds the worker gone ends the wait closed, as
    /// any does, and its journal line says which (#4332).
    #[test]
    fn a_refused_renewal_says_which_it_was() {
        fn refused(mut send: impl FnMut(&mut Waiting, &mut Worker) -> Result<()>) -> String {
            let directory = SessionDirectory::create().unwrap();
            let (mut worker, peer, input) = Worker::cooperative_fixture(&directory.path, "privacy");
            drop(peer);
            drop(input);
            let mut waiting = Waiting::new(new_boot().unwrap()).unwrap();
            // A child another test forks may hold the peer's socket until
            // its exec, so the first sends can still be taken (#1569).
            let deadline = Instant::now() + Duration::from_secs(5);
            let error = loop {
                waiting.last_publish = None;
                match send(&mut waiting, &mut worker) {
                    Err(error) => break error,
                    Ok(()) => {
                        assert!(Instant::now() < deadline, "no send was refused");
                        std::thread::sleep(Duration::from_millis(5));
                    }
                }
            };
            let _ = cleanup(&mut worker, None);
            drop(worker);
            let _ = fs::remove_dir(&directory.path);
            assert!(is_unavailable(&*error), "{error}");
            error.to_string()
        }
        let said = refused(|waiting, worker| waiting.ready(worker));
        assert!(said.ends_with(" (renewal after Ready)"), "{said}");
        let said =
            refused(|waiting, worker| waiting.renew(worker, None, 0, Renewal::InGate("controls")));
        assert!(
            said.ends_with(" (renewal in the gate after controls)"),
            "{said}"
        );
        let said = refused(|waiting, worker| {
            waiting
                .step(worker, None, &mut |_, _, _| Ok(()), &mut |_| Ok(false))
                .map(|_| ())
        });
        assert!(said.ends_with(" (renewal before the gate)"), "{said}");
        assert_eq!(
            Renewal::AfterGate.to_string(),
            "renewal after the gate",
            "the look's publish after its gate"
        );
    }
}
