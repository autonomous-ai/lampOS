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
    collections::VecDeque,
    fmt, fs, io,
    os::unix::fs::FileTypeExt,
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
        let cleanup = cleanup(&mut worker, None, &directory.path);
        drop(worker);
        let result = settled(Err(error), cleanup);
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

/// How long before the voice sees its own stop an indicator failure is still
/// the stop's (#4332). systemd's stop of `lamp-voice.service` signals the
/// whole unit at once, so the ring writer can die a moment before the voice
/// reads its own SIGTERM: on lamp-3e83 the stop was at 07:15:25.170 and the
/// failure was read and cleaned up by .209, about 40 ms. A second is 25
/// times that, room for a loaded board, and still far too short for a real
/// fault to pass as an update's stop by chance.
const STOP_WINDOW_US: u64 = 1_000_000;

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
    // A writer that failed and was reaped with its black write verified is
    // started once more, at most, in a wait (#4332).
    let mut retried = false;
    let mut indicator_failure = None;
    let mut indicator_failed_at = None;
    // When the failed writer was sent its Stop: a shutdown receipt must be
    // for a write after it.
    let mut indicator_stopped_at = 0;
    // The failed writer's own black write was read: its shutdown receipt,
    // or the control-lost one it writes as it ends on a fault.
    let mut indicator_blanked = false;
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
            if indicator_allowed
                && state.physical == PrivacyState::Muted
                && !attempted
                && indicator.is_none()
            {
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
                });
                let started = match started {
                    // A stop that came while the writer started is no fault
                    // (#4332): the start's own teardown has run.
                    Err(_) if termination.signal_name().is_some() => {
                        return Ok(WaitOutcome::Stopped);
                    }
                    other => other?,
                };
                indicator_blanked = false;
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
                        indicator_stopped_at = stop_failed(&mut waiting.ring_held, &mut child);
                        indicator = Some(child);
                    }
                }
            }
            let mut reaped = false;
            if let Some(child) = &mut indicator {
                if indicator_failure.is_none() {
                    // What the writer said is read before whether it still
                    // runs: a writer that sent its Fault and exited is
                    // failed by that Fault, not by its exit, which a loaded
                    // board can see first (ARM64 CI on #5244).
                    for _ in 0..16 {
                        match next_ring(&mut waiting.ring_held, child) {
                            Ok(Some(WorkerEvent::Ring {
                                report: report @ RingFeedback::Blanked { .. },
                            })) => {
                                // A writer ending on a fault writes black
                                // first and says so before its Fault.
                                if control_lost_black(report) {
                                    indicator_blanked = true;
                                }
                            }
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
                    if indicator_failure.is_none()
                        && let Err(error) = child.check_running()
                    {
                        indicator_failure = Some(error);
                    }
                    if let Some(error) = &indicator_failure {
                        indicator_failed_at = Some(monotonic_us());
                        eprintln!("lamp-voice: muted indication failed: {error}");
                        indicator_stopped_at = stop_failed(&mut waiting.ring_held, child);
                    }
                }
                if indicator_failure.is_some() {
                    // The failed writer is reaped here, in the wait, which
                    // stays muted with the microphones closed. Only once its
                    // black write is verified and its process gone may another
                    // writer start; without that black write the failure
                    // stays, and ends the wait as it always has.
                    drain_failed(
                        &mut waiting.ring_held,
                        child,
                        indicator_stopped_at,
                        &mut indicator_blanked,
                    );
                    match child.try_exit() {
                        Ok(Some(_)) => {
                            // A receipt can arrive between the drain and the
                            // exit: read once more, with the sender gone.
                            drain_failed(
                                &mut waiting.ring_held,
                                child,
                                indicator_stopped_at,
                                &mut indicator_blanked,
                            );
                            reaped = indicator_blanked && child.cleanup_ring_paths().is_ok();
                        }
                        Ok(None) => {
                            if !indicator_kill_requested
                                && indicator_failed_at
                                    .is_some_and(|at| monotonic_us().saturating_sub(at) >= 300_000)
                            {
                                indicator_kill_requested = true;
                                if let Err(error) = child.request_termination() {
                                    eprintln!(
                                        "lamp-voice: privacy indicator termination failed: {error}"
                                    );
                                }
                            }
                        }
                        Err(_) => {}
                    }
                }
            }
            if reaped {
                // A light that failed with no stop near never ends the voice
                // (#4332): the wait stays muted, and the writer is started
                // once more. If that one fails too, the wait goes on muted
                // without its light, and only the switch opening leaves it.
                indicator = None;
                waiting.ring_held.clear();
                indicator_failure = None;
                indicator_failed_at = None;
                indicator_kill_requested = false;
                indicator_blanked = false;
                if retried {
                    eprintln!(
                        "lamp-voice: the muted indication's writer ended, its black write verified; the wait stays muted without its light"
                    );
                } else {
                    eprintln!(
                        "lamp-voice: the muted indication's writer ended, its black write verified; the wait stays muted and starts it once more"
                    );
                    retried = true;
                    attempted = false;
                }
            }
            pass.done();
            std::thread::sleep(Duration::from_millis(2));
        }
    })();
    let stopped_at = termination.signal_name().map(|_| monotonic_us());
    let cleanup = cleanup_checked(&mut worker, indicator.as_deref_mut(), &directory.path);
    drop(indicator);
    drop(worker);
    // A stop asked for, seen by the end of the cleanup.
    let stopped_at = stopped_at.or_else(|| termination.signal_name().map(|_| monotonic_us()));
    // The indicator's trouble is the stop's when the stop came first, or
    // within the window after it failed (#4332, lamp-3e83): the update's
    // stop takes the writer down with the voice. The privacy worker's own
    // cleanup is never forgiven.
    let stop_explains = stopped_at.is_some_and(|stop| {
        indicator_failed_at.is_none_or(|at| stop.saturating_sub(at) <= STOP_WINDOW_US)
    });
    let result = if stop_explains && cleanup.privacy_verified {
        let said: Vec<String> = cleanup
            .result
            .err()
            .into_iter()
            .chain(indicator_failure)
            .map(|error| error.to_string())
            .collect();
        if !said.is_empty() {
            eprintln!(
                "lamp-voice: the muted indication ended with the stop, not a fault: {}",
                said.join("; ")
            );
        }
        result
    } else {
        let result = settled(result, cleanup.result);
        // Keep a failed writer's evidence distinct from permission to start a
        // new one, even when its process was subsequently reaped.
        finish(result, indicator_failure.map_or(Ok(()), Err))
    };
    finish(result, fs::remove_dir(&directory.path))
}

/// Sends a failed writer its Stop, and says when: its shutdown receipt must
/// be for a write after it. What it has queued is held first
/// ([`hold_events`]): a writer that said its black write and its Fault and
/// exited refuses this send, and on Linux the refusal would empty the wait's
/// queue, so [`drain_failed`] would never see the black write (the
/// Reviewer-lead's P2 on #5388).
fn stop_failed(held: &mut VecDeque<io::Result<WorkerEvent>>, child: &mut Worker) -> u64 {
    hold_events(held, child);
    let stopped_at = monotonic_us();
    let _ = child.channels.control.send(Control::Stop);
    stopped_at
}

/// Whether `report` is a black write a writer made as it ended on a fault
/// (`privacy_indicator::failed`): its last frame, written before its Fault.
fn control_lost_black(report: RingFeedback) -> bool {
    matches!(report, RingFeedback::Blanked {
        reason: RingBlankReason::ControlLost,
        write_started_at_us,
        write_finished_at_us,
    } if write_finished_at_us >= write_started_at_us
        && write_finished_at_us <= monotonic_us())
}

/// Reads a failed writer's events, without blocking, for its black write:
/// the shutdown receipt for a write after its Stop, or its own control-lost
/// one. Nothing else it says changes the failure.
fn drain_failed(
    held: &mut VecDeque<io::Result<WorkerEvent>>,
    child: &mut Worker,
    stopped_at: u64,
    blanked: &mut bool,
) {
    for _ in 0..16 {
        match next_ring(held, child) {
            Ok(Some(WorkerEvent::Ring {
                report: report @ RingFeedback::Blanked { .. },
            })) => {
                if report.confirms_shutdown(stopped_at, monotonic_us())
                    || control_lost_black(report)
                {
                    *blanked = true;
                }
            }
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => break,
        }
    }
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
    /// The worker's events read just before a renewal is sent, in order,
    /// for the next look to take first ([`Self::hold_queued`]).
    held: VecDeque<io::Result<WorkerEvent>>,
    /// The ring writer's events read just before a renewal is sent to it,
    /// in order, for the wait's own read of the writer to take first: the
    /// same Linux loss as [`Self::hold_queued`]'s, on the indicator's socket
    /// (ARM64 CI, #5224).
    ring_held: VecDeque<io::Result<WorkerEvent>>,
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
            held: VecDeque::new(),
            ring_held: VecDeque::new(),
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
            let next = match self.held.pop_front() {
                Some(held) => held.map(Some),
                None => worker.channels.control.receive::<WorkerEvent>(),
            };
            match next.map_err(unavailable)? {
                Some(WorkerEvent::Privacy {
                    acquired_at_us,
                    muted,
                    ..
                }) => {
                    self.sequence = self
                        .sequence
                        .checked_add(1)
                        .ok_or_else(|| io::Error::other("privacy observation count exhausted"))?;
                    // A stale observation only closes the gate, and nothing
                    // opens until a fresh one follows; it no longer ends the
                    // wait by itself (#5224: a muted boot's first look read
                    // one queued since Ready, with fresh ones right behind
                    // it). Whether anything fresh came is judged once the
                    // queue is drained, below, still failing closed.
                    self.gate.observe(
                        self.sequence,
                        MonoTime::from_micros(acquired_at_us),
                        MonoTime::from_micros(monotonic_us()),
                        muted,
                    );
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
        self.hold_queued(worker);
        let snapshot = self.controller.snapshot(MonoTime::from_micros(at))?;
        worker
            .channels
            .control
            .send(Control::Authority { snapshot })
            .map_err(|error| {
                unavailable(format!(
                    "{} ({which})",
                    own_fault(&self.held, worker, error)
                ))
            })?;
        if let Some(indicator) = indicator {
            hold_events(&mut self.ring_held, indicator);
            let _ = indicator
                .channels
                .control
                .send(Control::Authority { snapshot });
        }
        self.last_publish = Some(at);
        Ok(())
    }

    /// Reads what the worker has queued, without blocking, before a renewal
    /// is sent, and holds it for the next look, in order. On Linux a send
    /// refused because the worker's socket is gone empties this socket's
    /// own queue (af_unix `unix_dgram_disconnected`), so a worker's Fault
    /// queued before it exited would be lost to [`own_fault`]: the PR's test
    /// passed on macOS and failed on the ARM64 CI (#4332). At most 16 are
    /// held; reading stops at an error or a Fault, which the look then
    /// handles as it always did.
    fn hold_queued(&mut self, worker: &mut Worker) {
        hold_events(&mut self.held, worker);
    }
}

/// Reads what `worker` has queued into `held`, without blocking, before a
/// send to it ([`Waiting::hold_queued`]): at most 16, stopping at an error or
/// a Fault, and nothing more once one is held.
fn hold_events(held: &mut VecDeque<io::Result<WorkerEvent>>, worker: &mut Worker) {
    if matches!(held.back(), Some(Err(_) | Ok(WorkerEvent::Fault { .. }))) {
        return;
    }
    while held.len() < 16 {
        match worker.channels.control.receive::<WorkerEvent>() {
            Ok(None) => break,
            Ok(Some(event)) => {
                let fault = matches!(event, WorkerEvent::Fault { .. });
                held.push_back(Ok(event));
                if fault {
                    break;
                }
            }
            Err(error) => {
                held.push_back(Err(error));
                break;
            }
        }
    }
}

/// The ring writer's next event: one held before a renewal first
/// ([`Waiting::ring_held`]), then its socket's.
fn next_ring(
    held: &mut VecDeque<io::Result<WorkerEvent>>,
    child: &mut Worker,
) -> io::Result<Option<WorkerEvent>> {
    match held.pop_front() {
        Some(event) => event.map(Some),
        None => child.channels.control.receive::<WorkerEvent>(),
    }
}

/// Why a refused send to the privacy worker was refused: a worker that
/// ended on its own (an unreadable switch) sends its Fault, exits and unlinks
/// its socket, so the send finds only the socket gone (lamp-06c1, cold boot
/// 6, #4332). Its Fault, if already queued, is the reason; otherwise the
/// socket's error is, as before. The control channel is only read here,
/// without blocking: a refused send ends the wait closed, and nothing reads
/// this worker's events after it (the cleanup reads only its exit), so the
/// events passed over on the way to a Fault are not lost to later handling.
fn own_fault(
    held: &VecDeque<io::Result<WorkerEvent>>,
    worker: &mut Worker,
    refused: io::Error,
) -> String {
    // Held before the send: on Linux the refused send emptied the socket.
    if let Some(code) = held.iter().find_map(|event| match event {
        Ok(WorkerEvent::Fault { code }) => Some(code.clone()),
        _ => None,
    }) {
        return code;
    }
    // Queued since (macOS keeps a refused sender's queue).
    for _ in 0..16 {
        match worker.channels.control.receive::<WorkerEvent>() {
            Ok(Some(WorkerEvent::Fault { code })) => return code,
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => break,
        }
    }
    refused.to_string()
}

/// The privacy worker's own sockets in the wait's session directory, which
/// it unlinks itself as it ends (`lamp_ipc`'s binding drop).
const PRIVACY_WORKER_SOCKETS: [&str; 2] = ["privacy.c.w", "privacy.d.w"];

/// The wait's result with its workers' [`cleanup`]: an unreadable switch
/// from a killed and reaped privacy worker ([`killed_privacy_worker`]) is
/// the wait's own unreadable switch, read again after the retry; any other
/// cleanup failure is [`finish`]'s, as before. A wait that had already
/// failed keeps its own error, so a failure that ends the service still
/// does, and the kill is said on its own line.
fn settled<T>(result: Result<T>, cleanup: io::Result<()>) -> Result<T> {
    match cleanup {
        Err(killed) if is_unavailable(&killed) => Err(match result {
            Err(error) => {
                eprintln!("lamp-voice: {killed}");
                error
            }
            Ok(_) => killed.into(),
        }),
        other => finish(result, other),
    }
}

/// Send Stop to both workers before waiting. Both share one 300 ms userspace
/// grace, followed by explicit forced cleanup on expiry (always a failure,
/// except a privacy worker alone killed, reaped and its sockets removed:
/// [`killed_privacy_worker`]). `directory` is the wait's session directory,
/// where the privacy worker's sockets are.
fn cleanup(
    privacy: &mut Worker,
    indicator: Option<&mut Worker>,
    directory: &Path,
) -> io::Result<()> {
    cleanup_checked(privacy, indicator, directory).result
}

/// A [`cleanup`]'s result, and whether the privacy worker's own part of it
/// was verified: stopped in the grace, its exit clean. Only then can the
/// indicator's part be a stop's and not a fault (#4332).
struct Cleanup {
    result: io::Result<()>,
    privacy_verified: bool,
}

/// [`cleanup`], saying apart whether the privacy worker's part was verified.
fn cleanup_checked(
    privacy: &mut Worker,
    mut indicator: Option<&mut Worker>,
    directory: &Path,
) -> Cleanup {
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
    let mut killed = None;
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
    let mut privacy_failed = false;
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
                        privacy_failed = true;
                        error.get_or_insert(failure);
                    }
                }
                Ok(None) => {}
                Err(failure) => {
                    privacy_failed = true;
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
                killed = Some(privacy.terminate_and_reap(io::ErrorKind::TimedOut, reason));
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
    let privacy_verified = killed.is_none() && !privacy_failed;
    let result = match (killed, error) {
        (None, error) => error.map_or(Ok(()), Err),
        (Some(killed), None) => Err(killed_privacy_worker(privacy, directory, killed)),
        // Any other failure stays one, the kill said first as before.
        (Some(killed), Some(other)) => {
            Err(io::Error::new(killed.kind(), format!("{killed}; {other}")))
        }
    };
    Cleanup {
        result,
        privacy_verified,
    }
}

/// A privacy worker that did not stop within the grace and was killed
/// cannot unlink its own sockets, so the session directory could not be
/// removed and the voice exited 1 (lamp-06c1 cold boot, on4, #4332:
/// `reaped with signal: 9 (SIGKILL); verified cleanup failed: Directory not
/// empty`). Once it is reaped, no process owns them: they are removed, and
/// only they, only while each is a socket (never through a symlink), and the
/// kill is then an unreadable switch, read again with a fresh worker. On
/// the Controls route this worker only reads the bus and holds the wait's
/// Denied authority, never a microphone or a GPIO line, so its end grants
/// nothing. A worker not reaped, or a path that is not its socket, keeps
/// the kill a cleanup failure, with that path left as it is.
fn killed_privacy_worker(privacy: &mut Worker, directory: &Path, killed: io::Error) -> io::Error {
    if !matches!(privacy.try_exit(), Ok(Some(_))) {
        return killed;
    }
    let paths = PRIVACY_WORKER_SOCKETS.map(|name| directory.join(name));
    let mut present = Vec::new();
    for path in &paths {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_socket() => present.push(path),
            Ok(_) => {
                return io::Error::new(
                    killed.kind(),
                    format!("{killed}; {} is not its socket, left", path.display()),
                );
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return io::Error::new(killed.kind(), format!("{killed}; {error}"));
            }
        }
    }
    for path in present {
        if let Err(error) = fs::remove_file(path) {
            return io::Error::new(killed.kind(), format!("{killed}; {error}"));
        }
    }
    unavailable_io(killed)
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
        cleanup(&mut worker, None, &directory.path).unwrap();
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
                        cleanup(&mut worker, None, &fixtures.path),
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
        cleanup(&mut worker, None, &directory.path).unwrap();
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
            cleanup(&mut worker, None, &directory.path).unwrap();
            assert!(peer.join().unwrap().received_stop);
        }
    }

    /// The wait's look over `events`, queued on a cooperative fixture as
    /// the privacy worker would have sent them, and its fixture ended.
    fn first_look_over(events: &[(u64, bool)]) -> Result<Step> {
        let directory = SessionDirectory::create().unwrap();
        let (mut worker, mut peer, input) = Worker::cooperative_fixture(&directory.path, "privacy");
        let mut waiting = Waiting::new(new_boot().unwrap()).unwrap();
        let now = monotonic_us();
        for &(age_us, muted) in events {
            peer.control
                .send(WorkerEvent::Privacy {
                    acquired_at_us: now - age_us,
                    muted,
                    receipt: None,
                })
                .unwrap();
        }
        let step = waiting.step(&mut worker, None, &mut |_, _, _| Ok(()), &mut |_| Ok(false));
        drop(peer);
        drop(input);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !matches!(worker.try_exit(), Ok(Some(_))) {
            assert!(Instant::now() < deadline, "the fixture did not end");
            std::thread::sleep(Duration::from_millis(2));
        }
        worker.send_stop_until_peer_gone();
        cleanup(&mut worker, None, &directory.path).unwrap();
        drop(worker);
        let _ = fs::remove_dir(&directory.path);
        step
    }

    /// lamp-06c1, 12 Nov (#5224): a muted boot's first look read an
    /// observation queued since Ready, already past 50 ms, and the wait
    /// ended "invalid Controls privacy observation while waiting" although
    /// a fresh one was queued right behind it. The stale one only closes
    /// the gate; the fresh one behind it is read, and the wait goes on.
    #[test]
    fn a_stale_first_observation_with_a_fresh_one_behind_it_does_not_end_the_wait() {
        let step = first_look_over(&[(80_000, true), (0, true)])
            .expect("a fresh observation behind a stale one keeps the wait");
        assert_eq!(step.physical, PrivacyState::Muted);
        assert!(!step.allowed);
    }

    /// The same first look with nothing fresh behind the stale observation
    /// still fails closed, by the check after the queue is drained.
    #[test]
    fn a_stale_first_observation_alone_still_ends_the_wait_unavailable() {
        let error = first_look_over(&[(80_000, true)])
            .err()
            .expect("no fresh reading");
        assert!(is_unavailable(&*error), "{error}");
        assert!(
            error
                .to_string()
                .contains("Controls privacy unavailable while waiting"),
            "{error}"
        );
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

    /// A writer that started and failed before its Ready never showed the
    /// mute. Without its verified black write the failure ends the wait as
    /// it always has: the one indicator failure that stays fatal (#4332).
    /// With it, and its exit, the wait goes on muted and ends when the
    /// switch opens.
    #[test]
    fn a_started_ring_that_failed_still_needs_its_black_write_and_exit() {
        for receipts in [0, 1] {
            let fixtures = SessionDirectory::create().unwrap();
            let directory = SessionDirectory::create().unwrap();
            // The slide held muted until the failed writer has been dealt
            // with, then opened.
            let slide = Arc::new(AtomicBool::new(true));
            let (worker, peer) = crate::service_privacy::tests::mock_with_slide(
                &fixtures.path,
                Script::Muted,
                Arc::new(AtomicBool::new(false)),
                Some(Arc::clone(&slide)),
            );
            // Opened 200 ms after the writer was started, never before: on
            // a loaded host the wait can take longer than that to reach the
            // mute, and a slide opened first ends it before any writer.
            let started = Arc::new(AtomicBool::new(false));
            let opener = {
                let slide = Arc::clone(&slide);
                let started = Arc::clone(&started);
                std::thread::spawn(move || {
                    let deadline = Instant::now() + Duration::from_secs(10);
                    while !started.load(std::sync::atomic::Ordering::SeqCst)
                        && Instant::now() < deadline
                    {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    std::thread::sleep(Duration::from_millis(200));
                    slide.store(false, std::sync::atomic::Ordering::SeqCst);
                })
            };
            let rings = SessionDirectory::create().unwrap();
            let (ring, ring_peer) = serving_child(&rings.path, receipts);
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
                    started.store(true, std::sync::atomic::Ordering::SeqCst);
                    Ok(RingStartup::Unavailable {
                        worker: ring.take(),
                        error: io::Error::other("synthetic readiness failure after spawn"),
                    })
                },
            );
            if receipts == 0 {
                let error = outcome.unwrap_err().to_string();
                assert!(
                    error.contains("verified cleanup failed")
                        && error.contains("synthetic readiness failure after spawn"),
                    "{error}"
                );
            } else {
                assert!(matches!(outcome, Ok(WaitOutcome::Allowed)), "{outcome:?}");
            }
            opener.join().unwrap();
            assert!(peer.join().unwrap().received_stop);
            drop(ring_peer.join().unwrap());
        }
    }

    /// A ring writer stand-in that takes its controls as they come, on its
    /// own thread, for as long as the wait runs, and on Stop sends
    /// `receipts` shutdown receipts and ends. Its peer comes back once it
    /// has ended.
    fn serving_child(
        directory: &Path,
        receipts: usize,
    ) -> (
        Worker,
        std::thread::JoinHandle<crate::transport::WorkerChannels>,
    ) {
        let (worker, mut peer, input) = Worker::cooperative_fixture(directory, "ring");
        let task = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                match peer.control.receive::<Control>() {
                    Ok(Some(Control::Stop)) => {
                        for _ in 0..receipts {
                            let now = monotonic_us();
                            let _ = peer.control.send(WorkerEvent::Ring {
                                report: RingFeedback::Blanked {
                                    reason: RingBlankReason::Shutdown,
                                    write_started_at_us: now,
                                    write_finished_at_us: now,
                                },
                            });
                        }
                        break;
                    }
                    Ok(Some(_)) => continue,
                    Ok(None) | Err(_) => std::thread::sleep(Duration::from_millis(1)),
                }
            }
            drop(input);
            peer
        });
        (worker, task)
    }

    /// The black write a writer makes as it ends on a fault, as
    /// `privacy_indicator::failed` reports it before its Fault.
    fn control_lost() -> WorkerEvent {
        let now = monotonic_us();
        WorkerEvent::Ring {
            report: RingFeedback::Blanked {
                reason: RingBlankReason::ControlLost,
                write_started_at_us: now,
                write_finished_at_us: now,
            },
        }
    }

    /// The muted wait on a slide held muted, its writer started from
    /// `rings` in turn (none left: a writer that cannot start), and what
    /// `meanwhile` does to it once the first is up, given that writer's
    /// peer and input, the slide and a later SIGTERM. The outcome, and how
    /// many writers it started.
    fn a_muted_wait(
        rings: Vec<Worker>,
        meanwhile: impl FnOnce(
            crate::transport::WorkerChannels,
            std::process::ChildStdin,
            Arc<AtomicBool>,
            &dyn Fn(),
            &std::sync::mpsc::Receiver<usize>,
        ) + Send,
        first: (crate::transport::WorkerChannels, std::process::ChildStdin),
    ) -> (Result<WaitOutcome>, usize, Vec<PrivacyState>) {
        let fixtures = SessionDirectory::create().unwrap();
        let directory = SessionDirectory::create().unwrap();
        let slide = Arc::new(AtomicBool::new(true));
        let (worker, peer) = crate::service_privacy::tests::mock_with_slide(
            &fixtures.path,
            Script::Muted,
            Arc::new(AtomicBool::new(false)),
            Some(Arc::clone(&slide)),
        );
        let termination = Termination::inactive();
        let stop = termination.later();
        let (started, starts) = std::sync::mpsc::channel();
        let mut rings = std::collections::VecDeque::from(rings);
        let mut count = 0;
        let mut states = Vec::new();
        let outcome = std::thread::scope(|scope| {
            let (channels, input) = first;
            let slide = Arc::clone(&slide);
            let task = scope.spawn(move || {
                // The first writer is up.
                assert_eq!(starts.recv_timeout(Duration::from_secs(5)), Ok(1));
                meanwhile(channels, input, slide, &stop, &starts);
            });
            let outcome = run(
                worker,
                directory,
                Waiting::new(new_boot().unwrap()).unwrap(),
                &termination,
                |state, _, _| {
                    states.push(state);
                    Ok(())
                },
                // The software latch clear throughout: only the switch keeps
                // the wait muted.
                |_| Ok(true),
                true,
                |tick| {
                    tick()?;
                    count += 1;
                    let started_now = match rings.pop_front() {
                        Some(ring) => RingStartup::Ready(Box::new(ring)),
                        None => RingStartup::Unavailable {
                            worker: None,
                            error: io::Error::other("synthetic bind failure before spawn"),
                        },
                    };
                    let _ = started.send(count);
                    Ok(started_now)
                },
            );
            task.join().unwrap();
            outcome
        });
        assert!(peer.join().unwrap().received_stop);
        (outcome, count, states)
    }

    /// lamp-3e83, after every update (#4332): the update's stop took the
    /// muted indication's writer down with the voice, and its failure, read
    /// about 40 ms after systemd's stop, ended the voice with status 1, so
    /// the restart guard muted the next start. Now a writer that dies with
    /// the stop, or within the window before the voice sees it, is the
    /// stop's: the cleanup still runs, and the wait ends stopped, which the
    /// service ends with 0, its guard not armed.
    #[test]
    fn an_updates_stop_that_takes_the_light_down_is_no_fault() {
        for (stop_first, before) in [(true, Duration::ZERO), (false, Duration::from_millis(100))] {
            let rings = SessionDirectory::create().unwrap();
            let (ring, ring_peer, input) = Worker::cooperative_fixture(&rings.path, "ring");
            let (outcome, starts, _) = a_muted_wait(
                vec![ring],
                move |mut ring_peer, input, _, stop, _| {
                    std::thread::sleep(Duration::from_millis(50));
                    if stop_first {
                        stop();
                    }
                    // Taken down by the stop: its Fault, its end, and no
                    // black write read.
                    let _ = ring_peer.control.send(WorkerEvent::Fault {
                        code: "privacy ring: Transport endpoint is not connected".into(),
                    });
                    drop(ring_peer);
                    drop(input);
                    std::thread::sleep(before);
                    stop();
                },
                (ring_peer, input),
            );
            assert!(
                matches!(outcome, Ok(WaitOutcome::Stopped)),
                "stop_first={stop_first}: {outcome:?}"
            );
            assert_eq!(starts, 1, "no writer started after the stop");
        }
    }

    /// A writer that said its black write and its Fault, then exited, is
    /// read by both even when a renewal's send to its gone socket comes
    /// first: on Linux that refused send empties the wait's own queue, so the
    /// events are held before it (ARM64 CI, #5224; [`hold_events`]).
    #[test]
    fn a_writers_last_words_outlive_a_send_to_its_gone_socket() {
        let rings = SessionDirectory::create().unwrap();
        let (mut ring, mut ring_peer, input) = Worker::cooperative_fixture(&rings.path, "ring");
        ring_peer.control.send(control_lost()).unwrap();
        ring_peer
            .control
            .send(WorkerEvent::Fault {
                code: "privacy ring: synthetic SPI fault".into(),
            })
            .unwrap();
        let mut held = VecDeque::new();
        hold_events(&mut held, &mut ring);
        drop(ring_peer);
        drop(input);
        let _ = ring.channels.control.send(Control::Stop);
        let mut blanked = false;
        assert!(matches!(
            next_ring(&mut held, &mut ring),
            Ok(Some(WorkerEvent::Ring { report })) if control_lost_black(report)
        ));
        match next_ring(&mut held, &mut ring) {
            Ok(Some(WorkerEvent::Fault { code })) => assert!(code.contains("synthetic SPI fault")),
            other => panic!("the Fault was lost: {other:?}"),
        }
        hold_events(&mut held, &mut ring);
        drain_failed(&mut held, &mut ring, monotonic_us(), &mut blanked);
        assert!(!blanked, "nothing more after the Fault");
    }

    /// A failed writer's Stop goes to a socket it may have just left: its
    /// black write and Fault, said before it exited, are held before the
    /// send, so the drain still finds the black write verified (the
    /// Reviewer-lead's P2 on #5388; [`stop_failed`]).
    #[test]
    fn a_failed_writers_black_write_outlives_its_stop_to_a_gone_socket() {
        let rings = SessionDirectory::create().unwrap();
        let (mut ring, mut ring_peer, input) = Worker::cooperative_fixture(&rings.path, "ring");
        ring_peer.control.send(control_lost()).unwrap();
        ring_peer
            .control
            .send(WorkerEvent::Fault {
                code: "privacy ring: synthetic SPI fault".into(),
            })
            .unwrap();
        drop(ring_peer);
        drop(input);
        let mut held = VecDeque::new();
        let stopped_at = stop_failed(&mut held, &mut ring);
        let mut blanked = false;
        drain_failed(&mut held, &mut ring, stopped_at, &mut blanked);
        assert!(
            blanked,
            "the black write said before the exit is still read"
        );
    }

    /// A light that failed long before the stop is no stop's: past the
    /// window, its unverified black write ends the wait as it always has.
    #[test]
    fn a_light_that_failed_well_before_the_stop_is_still_a_fault() {
        let rings = SessionDirectory::create().unwrap();
        let (ring, ring_peer, input) = Worker::cooperative_fixture(&rings.path, "ring");
        let (outcome, starts, _) = a_muted_wait(
            vec![ring],
            |mut ring_peer, input, _, stop, _| {
                std::thread::sleep(Duration::from_millis(50));
                let _ = ring_peer.control.send(WorkerEvent::Fault {
                    code: "privacy ring: synthetic SPI fault".into(),
                });
                drop(ring_peer);
                drop(input);
                std::thread::sleep(
                    Duration::from_micros(STOP_WINDOW_US) + Duration::from_millis(300),
                );
                stop();
            },
            (ring_peer, input),
        );
        let error = outcome.unwrap_err().to_string();
        assert!(error.contains("synthetic SPI fault"), "{error}");
        assert_eq!(starts, 1, "no writer replaces one without its black write");
    }

    /// #4332, part 2: a writer that fails in the muted wait with no stop
    /// near, its black write verified, never ends the voice. The wait stays
    /// muted, its microphones closed (the privacy stand-in asserts every
    /// authority is Denied), and starts the writer once more. Whether that
    /// one comes back or not, the software latch's clear alone never leaves
    /// the wait: only the switch opening does.
    #[test]
    fn a_light_lost_with_no_stop_keeps_the_wait_muted_and_starts_it_once_more() {
        for comes_back in [false, true] {
            let rings = SessionDirectory::create().unwrap();
            let again = SessionDirectory::create().unwrap();
            let (ring, ring_peer, input) = Worker::cooperative_fixture(&rings.path, "ring");
            let mut writers = vec![ring];
            let mut second = None;
            if comes_back {
                let (ring, peer) = serving_child(&again.path, 1);
                writers.push(ring);
                second = Some(peer);
            }
            let opened = std::sync::Mutex::new(None);
            let (outcome, starts, states) = a_muted_wait(
                writers,
                |mut ring_peer, input, slide, _, starts| {
                    std::thread::sleep(Duration::from_millis(50));
                    ring_peer.control.send(control_lost()).unwrap();
                    ring_peer
                        .control
                        .send(WorkerEvent::Fault {
                            code: "privacy ring: synthetic heartbeat expired".into(),
                        })
                        .unwrap();
                    drop(input);
                    // Started once more, after the first was reaped.
                    assert_eq!(starts.recv_timeout(Duration::from_secs(5)), Ok(2));
                    // Still muted, and still waiting, long after.
                    std::thread::sleep(Duration::from_millis(200));
                    *opened.lock().unwrap() = Some(Instant::now());
                    slide.store(false, std::sync::atomic::Ordering::SeqCst);
                    drop(ring_peer);
                },
                (ring_peer, input),
            );
            let left = Instant::now();
            assert!(
                matches!(outcome, Ok(WaitOutcome::Allowed)),
                "comes_back={comes_back}: {outcome:?}"
            );
            assert!(
                opened.lock().unwrap().is_some_and(|at| at <= left),
                "the wait ended before the switch opened"
            );
            assert_eq!(starts, 2, "comes_back={comes_back}: one retry");
            assert!(states.contains(&PrivacyState::Muted));
            assert_eq!(states.last(), Some(&PrivacyState::Allowed));
            assert_eq!(
                states
                    .iter()
                    .filter(|state| **state == PrivacyState::Allowed)
                    .count(),
                1
            );
            if let Some(peer) = second {
                drop(peer.join().unwrap());
            }
        }
    }

    /// A writer that fails in the muted wait with no verified black write
    /// (no receipt, its process gone) is not replaced, and still ends the
    /// wait as an error when the switch opens, as before (#4332).
    #[test]
    fn a_light_lost_without_its_black_write_is_still_a_fault() {
        let rings = SessionDirectory::create().unwrap();
        let (ring, ring_peer, input) = Worker::cooperative_fixture(&rings.path, "ring");
        let (outcome, starts, _) = a_muted_wait(
            vec![ring],
            |mut ring_peer, input, slide, _, _| {
                std::thread::sleep(Duration::from_millis(50));
                ring_peer
                    .control
                    .send(WorkerEvent::Fault {
                        code: "privacy ring: synthetic SPI fault".into(),
                    })
                    .unwrap();
                drop(input);
                std::thread::sleep(Duration::from_millis(200));
                slide.store(false, std::sync::atomic::Ordering::SeqCst);
                drop(ring_peer);
            },
            (ring_peer, input),
        );
        // The failure named is the first seen: its Fault, or its exit.
        let error = outcome.unwrap_err().to_string();
        assert!(error.contains("verified cleanup failed"), "{error}");
        assert_eq!(starts, 1);
    }

    #[test]
    fn next_writer_needs_one_black_receipt_and_both_verified_exits() {
        for receipts in [0, 1, 2] {
            let directory = SessionDirectory::create().unwrap();
            let (mut privacy, privacy_peer) = stopping_child(&directory.path, "privacy", 0);
            let (mut indicator, ring_peer) = stopping_child(&directory.path, "ring", receipts);
            let result = cleanup(&mut privacy, Some(&mut indicator), &directory.path);
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

    /// A privacy worker that ends itself as the real one did when its
    /// controller's lease lapses (`privacy controller lease expired`): no
    /// authority within 250 ms of its start, or the one it holds expired.
    /// The real one now rides a lapse of a Denied-only lease
    /// (`privacy_bus::DeniedLapse`, #4332); this strict stand-in measures
    /// the wait's renewals, which still keep the lease from lapsing at all.
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
        let _ = cleanup(&mut worker, None, &directory.path);
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
        let _ = cleanup(&mut worker, None, &directory.path);
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
        let _ = cleanup(&mut worker, None, &directory.path);
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

    /// The privacy worker now rides a lapse of the wait's Denied lease
    /// (`privacy_bus::DeniedLapse`, #4332), but a worker that ended
    /// abnormally during the wait is still a real loss: the wait ends with
    /// an error that is not an unreadable switch, so the voice exits and its
    /// restart guard counts it, as on 06c1 before the ride.
    #[test]
    fn a_privacy_worker_that_died_in_the_wait_still_ends_it_for_the_guard() {
        let fixtures = SessionDirectory::create().unwrap();
        let directory = SessionDirectory::create().unwrap();
        let (worker, _peer) = Worker::command_fixture(&fixtures.path, "privacy", "exit 1");
        let outcome = run(
            worker,
            directory,
            Waiting::new(new_boot().unwrap()).unwrap(),
            &Termination::inactive(),
            |_, _, _| Ok(()),
            |_| Ok(false),
            false,
            |_| unreachable!("no indicator on this route"),
        );
        let error = outcome.expect_err("the wait went on past a dead worker");
        assert!(!is_unavailable(&*error), "{error}");
        assert!(error.to_string().contains("exited abnormally"), "{error}");
        let _ = fs::remove_dir(&fixtures.path);
    }

    /// A privacy worker that ignores its Stop, in `directory`, its sockets
    /// left behind as a killed process leaves them: its peer's endpoints,
    /// which would unlink them, are never dropped.
    fn a_stalled_privacy_worker(directory: &Path) -> Worker {
        let (worker, peer) = Worker::sleeping_fixture(directory, "privacy");
        std::mem::forget(peer);
        worker
    }

    /// A wait whose look failed on an unreadable switch, ended by [`run`]
    /// with `worker` in `directory`.
    fn a_wait_ended(worker: Worker, directory: SessionDirectory) -> Result<WaitOutcome> {
        run(
            worker,
            directory,
            Waiting::new(new_boot().unwrap()).unwrap(),
            &Termination::inactive(),
            |_, _, _| Ok(()),
            |_| Err(unavailable("Controls privacy unavailable while waiting")),
            false,
            |_| unreachable!("no indicator on this route"),
        )
    }

    /// lamp-06c1's cold boot, on4 (#4332): the wait failed on an unreadable
    /// switch, its privacy worker did not stop within 300 ms and was
    /// killed, and its `privacy.c.w` and `privacy.d.w` were left, so the
    /// session directory was `not empty` and the voice exited 1. Now the
    /// reaped worker's sockets are removed, the directory with them, and the
    /// wait is an unreadable switch, read again after the retry.
    #[test]
    fn a_killed_privacy_workers_sockets_go_and_the_switch_is_read_again() {
        let directory = SessionDirectory::create().unwrap();
        let path = directory.path.clone();
        let worker = a_stalled_privacy_worker(&path);
        let error = a_wait_ended(worker, directory).expect_err("the wait failed");
        assert!(is_unavailable(&*error), "{error}");
        assert!(!path.exists(), "the session directory was left");
        let mut first = Some(error);
        let mut waits = 0;
        let outcome = retried(&Termination::inactive(), Duration::from_millis(1), || {
            waits += 1;
            first.take().map_or(Ok(WaitOutcome::Allowed), Err)
        })
        .unwrap();
        assert_eq!((outcome, waits), (WaitOutcome::Allowed, 2));
        // A look that had allowed is read again too: the kill is never
        // passed over as verified cleanup.
        let directory = SessionDirectory::create().unwrap();
        let mut worker = a_stalled_privacy_worker(&directory.path);
        let cleanup = cleanup(&mut worker, None, &directory.path);
        let error = settled(Ok(WaitOutcome::Allowed), cleanup).expect_err("not allowed");
        assert!(is_unavailable(&*error), "{error}");
        assert!(error.to_string().contains("SIGKILL"), "{error}");
        drop(worker);
        fs::remove_dir(&directory.path).unwrap();
    }

    /// Only the killed worker's own sockets are removed: a regular file, or
    /// a symlink even to a socket, at either path is left as it is, and the
    /// kill stays a cleanup failure that ends the service.
    #[test]
    fn a_path_that_is_not_the_killed_workers_socket_is_left_and_still_fails() {
        let outside = SessionDirectory::create().unwrap();
        let target = outside.path.join("kept");
        let listener = std::os::unix::net::UnixDatagram::bind(&target).unwrap();
        for (name, symlink) in [("privacy.d.w", false), ("privacy.c.w", true)] {
            let directory = SessionDirectory::create().unwrap();
            let path = directory.path.clone();
            let worker = a_stalled_privacy_worker(&path);
            let replaced = path.join(name);
            fs::remove_file(&replaced).unwrap();
            if symlink {
                std::os::unix::fs::symlink(&target, &replaced).unwrap();
            } else {
                fs::write(&replaced, b"not a socket").unwrap();
            }
            let error = a_wait_ended(worker, directory).expect_err("the wait failed");
            assert!(!is_unavailable(&*error), "{name}: {error}");
            assert!(error.to_string().contains("is not its socket"), "{error}");
            let kind = fs::symlink_metadata(&replaced).unwrap().file_type();
            assert_eq!((kind.is_symlink(), kind.is_file()), (symlink, !symlink));
            assert!(
                fs::symlink_metadata(&target)
                    .unwrap()
                    .file_type()
                    .is_socket()
            );
            for left in PRIVACY_WORKER_SOCKETS {
                let _ = fs::remove_file(path.join(left));
            }
            fs::remove_dir(&path).unwrap();
        }
        drop(listener);
        fs::remove_file(&target).unwrap();
    }

    /// A privacy worker that stops within the grace is cleaned up as
    /// before: its sockets are its own to unlink, and none is touched.
    #[test]
    fn a_privacy_worker_that_stops_keeps_its_own_sockets_to_unlink() {
        let directory = SessionDirectory::create().unwrap();
        let (mut worker, peer) = stopping_child(&directory.path, "privacy", 0);
        cleanup(&mut worker, None, &directory.path).unwrap();
        assert!(worker.try_exit().unwrap().unwrap().success());
        for name in PRIVACY_WORKER_SOCKETS {
            let kind = fs::symlink_metadata(directory.path.join(name)).unwrap();
            assert!(kind.file_type().is_socket(), "{name}");
        }
        drop(peer.join().unwrap());
        drop(worker);
        fs::remove_dir(&directory.path).unwrap();
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
            let _ = cleanup(&mut worker, None, &directory.path);
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

    /// A worker that ended on its own sent its Fault, exited and unlinked its
    /// socket before the first authority: the refused renewal says the
    /// worker's reason, not the socket's Connection refused (lamp-06c1, cold
    /// boot 6, #4332).
    #[test]
    fn a_refused_renewal_says_the_workers_own_fault() {
        let directory = SessionDirectory::create().unwrap();
        let (mut worker, mut peer, input) = Worker::cooperative_fixture(&directory.path, "privacy");
        peer.control
            .send(WorkerEvent::Fault {
                code: "physical privacy source expired".into(),
            })
            .unwrap();
        drop(peer);
        drop(input);
        let mut waiting = Waiting::new(new_boot().unwrap()).unwrap();
        // A child another test forks may hold the peer's socket until its
        // exec, so the first sends can still be taken (#1569).
        let deadline = Instant::now() + Duration::from_secs(5);
        let error = loop {
            waiting.last_publish = None;
            match waiting.ready(&mut worker) {
                Err(error) => break error,
                Ok(()) => {
                    assert!(Instant::now() < deadline, "no send was refused");
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        };
        let _ = cleanup(&mut worker, None, &directory.path);
        drop(worker);
        let _ = fs::remove_dir(&directory.path);
        assert!(is_unavailable(&*error), "{error}");
        let said = error.to_string();
        assert!(
            said.contains("physical privacy source expired (renewal after Ready)"),
            "{said}"
        );
        // Linux says Connection refused; macOS, Connection reset by peer.
        assert!(!said.contains("Connection"), "{said}");
    }
}
