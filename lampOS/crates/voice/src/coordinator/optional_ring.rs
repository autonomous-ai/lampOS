//! Disable failed presentation immediately; observe cleanup without waiting on it.
use super::*;
use crate::process::RingStartup;

const STOP_GRACE_US: u64 = 300_000;
const CLEANUP_EVENT_BUDGET: usize = 16;
/// A ring that faults is started again at most this many times a session
/// (#1620). Past that it stays off, and the voice's health says so.
pub(super) const RING_RESTARTS: u32 = 3;
/// A restart comes no sooner than this after the fault it follows, so
/// restarts are at least this far apart.
pub(super) const RESTART_SPACING_US: u64 = 2_000_000;

pub(super) struct OptionalRing {
    worker: Option<Worker>,
    degraded: Option<Degraded>,
    startup_events: Vec<Value>,
    startup_events_dropped: u64,
    /// How a ring that started is started again; none for a ring that never
    /// started, or a test's fixture.
    restart: Option<Restart>,
}

/// The ring's own startup ([`Worker::spawn_optional_ring_with_tick`]), run
/// again on a thread of its own so the coordinator's loop never waits on it.
struct Restart {
    executable: PathBuf,
    directory: PathBuf,
    parent: BootId,
    extra: Vec<String>,
    /// Restarts begun this session.
    begun: u32,
    pending: Option<std::thread::JoinHandle<io::Result<RingStartup>>>,
    /// The ring was said to stay off.
    off_said: bool,
}

struct Degraded {
    at_us: u64,
    cause: String,
    notice_pending: bool,
    stop_error: Option<String>,
    cleanup_error: Option<String>,
    blank_confirmed: bool,
    exit_observed: bool,
    kill_requested: bool,
}

impl From<Option<Worker>> for OptionalRing {
    fn from(worker: Option<Worker>) -> Self {
        Self {
            worker,
            degraded: None,
            startup_events: Vec::new(),
            startup_events_dropped: 0,
            restart: None,
        }
    }
}

impl OptionalRing {
    pub(super) fn spawn(
        executable: &Path,
        directory: &Path,
        parent: BootId,
        extra: &[&str],
        on_wait: impl FnMut() -> io::Result<()>,
    ) -> io::Result<Self> {
        match Worker::spawn_optional_ring_with_tick(executable, directory, parent, extra, on_wait)?
        {
            RingStartup::Ready(worker) => {
                let mut ring = Self::from(Some(*worker));
                ring.restart = Some(Restart {
                    executable: executable.to_owned(),
                    directory: directory.to_owned(),
                    parent,
                    extra: extra.iter().map(|&argument| argument.to_owned()).collect(),
                    begun: 0,
                    pending: None,
                    off_said: false,
                });
                Ok(ring)
            }
            RingStartup::Unavailable { worker, error } => {
                let mut ring = Self::from(worker.map(|worker| *worker));
                ring.latch_disabled(format!("ring startup failed: {error}"));
                Ok(ring)
            }
        }
    }

    pub(super) fn active_mut(&mut self) -> Option<&mut Worker> {
        if self.degraded.is_none() {
            self.worker.as_mut()
        } else {
            None
        }
    }

    /// Includes the disabled child: losing availability never loses ownership.
    pub(super) fn iter_mut(&mut self) -> std::option::IterMut<'_, Worker> {
        self.worker.iter_mut()
    }

    pub(super) fn is_some(&self) -> bool {
        self.worker.is_some()
    }
    pub(super) fn is_active(&self) -> bool {
        self.worker.is_some() && self.degraded.is_none()
    }

    /// Disabled, and its process seen ended before now: a later look at the
    /// same (cached) exit status is not a new exit.
    pub(super) fn exit_observed(&self) -> bool {
        self.degraded.as_ref().is_some_and(|d| d.exit_observed)
    }

    pub(super) fn blank_confirmed(&self) -> bool {
        self.worker.is_none() || self.degraded.as_ref().is_some_and(|d| d.blank_confirmed)
    }

    pub(super) fn stop_requested_at(&self, normal_shutdown_at: u64) -> u64 {
        self.degraded
            .as_ref()
            .map_or(normal_shutdown_at, |d| d.at_us)
    }

    pub(super) fn cleanup_error(&self) -> Option<io::Error> {
        self.degraded
            .as_ref()
            .and_then(|d| d.cleanup_error.as_ref())
            .map(|message| io::Error::other(message.clone()))
    }

    pub(super) fn disable(&mut self, cause: impl std::fmt::Display) {
        if self.degraded.is_some() || self.worker.is_none() {
            return;
        }
        self.latch_disabled(cause);
    }

    fn latch_disabled(&mut self, cause: impl std::fmt::Display) {
        // Latch before attempting any I/O. No subsequent authority or frame can
        // reach this child, even when its Stop socket is full or disconnected.
        self.degraded = Some(Degraded {
            at_us: monotonic_us(),
            cause: cause.to_string(),
            notice_pending: true,
            stop_error: None,
            cleanup_error: None,
            blank_confirmed: false,
            exit_observed: false,
            kill_requested: false,
        });
        if let Some(worker) = self.worker.as_mut()
            && let Err(error) = worker.channels.control.send(Control::Stop)
        {
            let state = self.degraded.as_mut().unwrap();
            state.stop_error = Some(error.to_string());
            state.cleanup_error = Some(format!("ring stop delivery failed: {error}"));
        }
    }

    pub(super) fn publish(&mut self, snapshot: Snapshot) {
        if let Some(worker) = self.active_mut()
            && let Err(error) = worker
                .channels
                .control
                .send(Control::Authority { snapshot })
        {
            self.disable(format!("ring authority transport failed: {error}"));
        }
    }

    pub(super) fn check_running(&mut self) {
        if let Some(worker) = self.active_mut()
            && let Err(error) = worker.check_running()
        {
            self.disable(error);
        }
    }

    /// A ring that faulted and will not be started again this session: it
    /// stays off, and the voice's health says so.
    pub(super) fn off_for_good(&self) -> bool {
        self.degraded.is_some()
            && self
                .restart
                .as_ref()
                .is_none_or(|restart| restart.begun >= RING_RESTARTS && restart.pending.is_none())
    }

    /// Starts a faulted ring again, at most [`RING_RESTARTS`] times a
    /// session and [`RESTART_SPACING_US`] after its fault, once its process
    /// has exited, so two never write to the ring at once. The startup runs
    /// on a thread: this only looks at it, and never waits. True when a
    /// restarted ring has just become the active one: it wrote its black
    /// first, and takes the current authority with the next publish.
    pub(super) fn restart_tick(&mut self, trace: &mut SessionState) -> Result<bool> {
        self.restart_tick_with_clock(|event| record(trace, event), monotonic_us)
    }

    pub(super) fn restart_tick_with_clock(
        &mut self,
        mut emit: impl FnMut(Value) -> Result<()>,
        mut clock: impl FnMut() -> u64,
    ) -> Result<bool> {
        let Some(restart) = self.restart.as_mut() else {
            return Ok(false);
        };
        if let Some(pending) = &restart.pending {
            if !pending.is_finished() {
                return Ok(false);
            }
            return self.adopt(&mut emit, clock());
        }
        let Some(state) = &self.degraded else {
            return Ok(false);
        };
        let exited = state.exit_observed || self.worker.is_none();
        let at_us = clock();
        if restart.begun >= RING_RESTARTS {
            if !restart.off_said {
                restart.off_said = true;
                eprintln!(
                    "lamp-voice: the ring faulted again after {RING_RESTARTS} restarts ({}); it stays off this session",
                    state.cause
                );
                emit(
                    json!({"kind":"ring_off", "at_us":at_us, "restarts":restart.begun,
                    "reason":state.cause}),
                )?;
            }
            return Ok(false);
        }
        if !exited || at_us < state.at_us.saturating_add(RESTART_SPACING_US) {
            return Ok(false);
        }
        restart.begun += 1;
        let attempt = restart.begun;
        eprintln!(
            "lamp-voice: the ring faulted ({}); starting it again, {attempt} of {RING_RESTARTS}",
            state.cause
        );
        emit(
            json!({"kind":"ring_restart", "at_us":at_us, "attempt":attempt,
            "of":RING_RESTARTS, "after_fault_ms":at_us.saturating_sub(state.at_us) / 1_000,
            "reason":state.cause}),
        )?;
        // Its exit was observed: dropping it reaps it and removes its
        // namespace, which the restart then makes afresh. That is file work,
        // so it is done on the restart's thread, just before the startup: a
        // lamp whose disk or memory stalls never holds the loop here
        // (Reviewer-two's P3 on #4658). What its cleanup could not confirm
        // was recorded; the restarted ring's own stop is what a stop checks.
        let old = self.worker.take();
        self.degraded = None;
        let (executable, directory, parent, extra) = (
            restart.executable.clone(),
            restart.directory.clone(),
            restart.parent,
            restart.extra.clone(),
        );
        restart.pending = Some(std::thread::spawn(move || {
            drop(old);
            let extra: Vec<&str> = extra.iter().map(String::as_str).collect();
            Worker::spawn_optional_ring_with_tick(
                &executable,
                &directory,
                parent,
                &extra,
                || Ok(()),
            )
        }));
        Ok(false)
    }

    /// Takes a finished restart: the active ring if it started, a disabled
    /// one otherwise, to be tried again after the spacing.
    fn adopt(&mut self, emit: &mut impl FnMut(Value) -> Result<()>, at_us: u64) -> Result<bool> {
        let restart = self.restart.as_mut().expect("restart");
        let attempt = restart.begun;
        let started = restart
            .pending
            .take()
            .expect("pending restart")
            .join()
            .unwrap_or_else(|_| Err(io::Error::other("ring restart thread panicked")));
        match started {
            Ok(RingStartup::Ready(worker)) => {
                self.worker = Some(*worker);
                self.degraded = None;
                eprintln!("lamp-voice: the ring is back ({attempt} of {RING_RESTARTS} restarts)");
                emit(
                    json!({"kind":"ring_restarted", "at_us":at_us, "attempt":attempt,
                    "worker_boot":self.worker.as_ref().map(|worker| worker.boot)}),
                )?;
                Ok(true)
            }
            Ok(RingStartup::Unavailable { worker, error }) => {
                self.worker = worker.map(|worker| *worker);
                self.latch_disabled(format!("ring restart failed: {error}"));
                Ok(false)
            }
            Err(error) => {
                self.worker = None;
                self.latch_disabled(format!("ring restart failed: {error}"));
                Ok(false)
            }
        }
    }

    /// A restart's startup thread is still running.
    #[cfg(test)]
    pub(super) fn restart_pending(&self) -> bool {
        self.restart
            .as_ref()
            .is_some_and(|restart| restart.pending.is_some())
    }

    /// Before a stop: a restart under way is waited for (its startup is
    /// bounded), so a ring it started is stopped and writes its black like
    /// any other, and none is left behind.
    pub(super) fn settle_restart(&mut self, trace: &mut SessionState) -> Result<()> {
        if let Some(restart) = self.restart.as_mut()
            && let Some(pending) = restart.pending.take()
        {
            while !pending.is_finished() {
                std::thread::sleep(Duration::from_millis(2));
            }
            restart.pending = Some(pending);
            self.adopt(&mut |event| record(trace, event), monotonic_us())?;
        }
        Ok(())
    }

    /// Pre-readiness ring observations have a separate fixed allowance. Optional
    /// evidence cannot exhaust the required workers' startup-event budget.
    pub(super) fn startup_tick(&mut self, snapshot: Option<Snapshot>) {
        if self.is_active() {
            for _ in 0..CLEANUP_EVENT_BUDGET {
                let received = self.worker.as_mut().unwrap().channels.control.receive();
                match received {
                    Ok(Some(
                        event @ WorkerEvent::Ring {
                            report: RingFeedback::Blanked { .. },
                        },
                    )) => {
                        self.defer_startup(json!({"kind":"ring_startup_event", "at_us":monotonic_us(), "details":event}));
                    }
                    Ok(None) => break,
                    Ok(Some(event)) => {
                        self.disable(format!("unexpected ring startup event: {event:?}"));
                        break;
                    }
                    Err(error) => {
                        self.disable(error);
                        break;
                    }
                }
            }
        }
        self.check_running();
        if let Some(snapshot) = snapshot {
            self.publish(snapshot);
        }
        let mut pending = std::mem::take(&mut self.startup_events);
        let mut dropped = self.startup_events_dropped;
        // This emitter cannot fail; cleanup and mandatory progress never wait on
        // optional metadata. Lost observations get an explicit count at activation.
        let _ = self.poll_cleanup_with_clock(
            |event| {
                if pending.len() < CLEANUP_EVENT_BUDGET {
                    pending.push(event);
                } else {
                    dropped = dropped.saturating_add(1);
                }
                Ok(())
            },
            monotonic_us,
        );
        self.startup_events = pending;
        self.startup_events_dropped = dropped;
    }

    fn defer_startup(&mut self, event: Value) {
        if self.startup_events.len() < CLEANUP_EVENT_BUDGET {
            self.startup_events.push(event);
        } else {
            self.startup_events_dropped = self.startup_events_dropped.saturating_add(1);
        }
    }

    /// At most sixteen receives, one waitpid(WNOHANG), and one signal attempt.
    /// This never joins, sleeps, restarts or performs hardware I/O. A kernel
    /// signal/waitpid call has no userspace hard-deadline guarantee.
    pub(super) fn poll_cleanup(&mut self, trace: &mut SessionState) -> Result<()> {
        if self.startup_events_dropped != 0 {
            record(
                trace,
                json!({"kind":"ring_startup_observations_dropped", "count":self.startup_events_dropped}),
            )?;
            self.startup_events_dropped = 0;
        }
        for event in self.startup_events.drain(..) {
            record(trace, event)?;
        }
        self.poll_cleanup_with_clock(|event| record(trace, event), monotonic_us)
    }

    fn poll_cleanup_with_clock(
        &mut self,
        mut emit: impl FnMut(Value) -> Result<()>,
        mut clock: impl FnMut() -> u64,
    ) -> Result<()> {
        let Some(state) = &mut self.degraded else {
            return Ok(());
        };
        if state.notice_pending {
            emit(json!({"kind":"ring_degraded", "at_us":state.at_us,
                "worker_boot":self.worker.as_ref().map(|worker| worker.boot),
                "process_started":self.worker.is_some(), "reason":state.cause,
                "stop_delivery_error":state.stop_error, "audio_continues":true,
                "light_available":false, "cleanup_confirmed":false}))?;
            state.notice_pending = false;
        }
        let Some(worker) = &mut self.worker else {
            return Ok(());
        };
        for _ in 0..CLEANUP_EVENT_BUDGET {
            match worker.channels.control.receive::<WorkerEvent>() {
                Ok(Some(WorkerEvent::Ring {
                    report:
                        report @ RingFeedback::Blanked {
                            reason: RingBlankReason::Shutdown,
                            ..
                        },
                })) => {
                    let at_us = clock();
                    let valid =
                        !state.blank_confirmed && report.confirms_shutdown(state.at_us, at_us);
                    if valid {
                        state.blank_confirmed = true;
                    } else {
                        state
                            .cleanup_error
                            .get_or_insert_with(|| "invalid degraded ring shutdown receipt".into());
                    }
                    emit(json!({"kind":"ring_degraded_blank", "at_us":at_us,
                        "details":report, "valid":valid,
                        "boundary":"SPI write return; optical darkness unmeasured"}))?;
                }
                Ok(Some(event)) => {
                    let at_us = clock();
                    if !matches!(event, WorkerEvent::Ring { .. }) {
                        state.cleanup_error.get_or_insert_with(|| {
                            format!("unexpected degraded ring event: {event:?}")
                        });
                    }
                    // Old presentation receipts remain historical. They cannot
                    // reach choreography or signal listening readiness.
                    emit(json!({"kind":"ring_degraded_event", "at_us":at_us, "details":event}))?;
                }
                Ok(None) => break,
                Err(error) => {
                    state
                        .cleanup_error
                        .get_or_insert_with(|| format!("ring cleanup transport failed: {error}"));
                    break;
                }
            }
        }
        if !state.exit_observed {
            match worker.try_exit() {
                Ok(Some(status)) => {
                    let at_us = clock();
                    state.exit_observed = true;
                    let exit_error = worker
                        .exit_result(status)
                        .err()
                        .map(|error| error.to_string());
                    if let Some(error) = &exit_error {
                        state.cleanup_error.get_or_insert_with(|| error.clone());
                    }
                    emit(json!({"kind":"ring_degraded_exit", "at_us":at_us,
                        "success":status.success(), "status":status.to_string(),
                        "exit_validation_error":exit_error,
                        "blank_write_confirmed":state.blank_confirmed}))?;
                }
                Ok(None) => {}
                Err(error) => {
                    state
                        .cleanup_error
                        .get_or_insert_with(|| format!("ring cleanup status failed: {error}"));
                }
            }
        }
        let at_us = clock();
        if !state.exit_observed
            && !state.kill_requested
            && at_us.saturating_sub(state.at_us) >= STOP_GRACE_US
        {
            state.kill_requested = true;
            let result = worker.request_termination();
            state.cleanup_error.get_or_insert_with(|| {
                "ring exceeded cooperative stop grace; forced termination requested".into()
            });
            emit(json!({"kind":"ring_degraded_kill_requested", "at_us":at_us,
                "error":result.err().map(|e| e.to_string()), "exit_confirmed":false}))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_optional_ring_executable_does_not_end_required_startup() {
        let directory = SessionDirectory::create().unwrap();
        let mut ticks = 0;
        let result = OptionalRing::spawn(
            &directory.path.join("missing-executable"),
            &directory.path,
            new_boot().unwrap(),
            &[],
            || {
                ticks += 1;
                Ok(())
            },
        );
        assert!(result.is_ok(), "optional ring failure ended voice startup");
        let mut ring = result.unwrap();
        assert!(!ring.is_active());
        assert!(ring.active_mut().is_none());
        assert!(ticks > 0, "required startup supervision was skipped");
        let mut trace = SessionState::default();
        ring.poll_cleanup(&mut trace).unwrap();
        assert_eq!(
            trace
                .iter()
                .filter(|e| e["kind"] == "ring_degraded")
                .count(),
            1
        );
        assert!(
            trace
                .iter()
                .any(|e| e["kind"] == "ring_degraded" && e["audio_continues"] == true)
        );
        // Kept in test output so a target/emulator run can distinguish an exec
        // error with no child from an owned child that reports a later exit.
        for event in trace.iter() {
            eprintln!("missing-executable lifecycle: {event}");
        }
    }

    #[test]
    fn failed_startup_is_inhibited_through_late_ready_and_bounded_reaping() {
        use crate::process::ring_startup_tests::{ChildFixture, await_exit};
        let fixture = ChildFixture::new();
        let mut peer = None;
        let mut ring = OptionalRing::spawn(
            &fixture.executable,
            &fixture.directory.path,
            new_boot().unwrap(),
            &[],
            || {
                if peer.is_none() {
                    peer = fixture.connect_peer();
                    if let Some(peer) = &mut peer {
                        peer.control.send(WorkerEvent::Fault {
                            code: "fake SPI start failure".into(),
                        })?;
                    }
                }
                Ok(())
            },
        )
        .unwrap();
        assert!(!ring.is_active());
        assert!(ring.is_some());
        let peer = peer.as_mut().unwrap();
        assert!(matches!(
            peer.control.receive().unwrap(),
            Some(Control::Stop)
        ));
        peer.control.send(WorkerEvent::Ready).unwrap();
        let mut owner = Controller::new(new_boot().unwrap(), now());
        ring.startup_tick(Some(owner.snapshot(now()).unwrap()));
        assert!(!ring.is_active());
        assert!(peer.control.receive::<Control>().unwrap().is_none());
        assert!(!owner.snapshot(now()).unwrap().listening_ready(now()));
        let at = ring.stop_requested_at(0);
        let mut trace = SessionState::default();
        ring.poll_cleanup_with_clock(|event| record(&mut trace, event), || at + STOP_GRACE_US)
            .unwrap();
        assert!(ring.degraded.as_ref().unwrap().kill_requested);
        await_exit(ring.iter_mut().next().unwrap());
        ring.poll_cleanup(&mut trace).unwrap();
        assert!(ring.degraded.as_ref().unwrap().exit_observed);
        assert!(!ring.blank_confirmed());
        assert!(ring.cleanup_error().is_some());
        assert_eq!(
            trace
                .iter()
                .filter(|e| e["kind"] == "ring_degraded_kill_requested")
                .count(),
            1
        );
        assert!(
            !trace
                .iter()
                .any(|e| e["kind"] == "ring_requested" || e["kind"] == "listening_ready")
        );
        fixture.assert_reaped();
        drop(ring);
        assert!(!fixture.directory.path.join("ring").exists());
    }

    #[test]
    fn optional_startup_observations_are_bounded_without_required_budget_pressure() {
        let directory = SessionDirectory::create().unwrap();
        // Force failure before child creation. A missing executable alone is
        // not a portable proof that no child was returned by spawn (for example
        // under a process emulator); an owned exit is another valid event.
        let ring_directory = directory.path.join("ring");
        fs::create_dir(&ring_directory).unwrap();
        let mut ring = OptionalRing::spawn(
            &directory.path.join("absent"),
            &directory.path,
            new_boot().unwrap(),
            &[],
            || Ok(()),
        )
        .unwrap();
        assert!(!ring.is_some(), "namespace failure must not create a child");
        for index in 0..CLEANUP_EVENT_BUDGET + 3 {
            ring.defer_startup(json!({"kind":"test_ring_observation", "index":index}));
        }
        ring.startup_tick(None);
        assert_eq!(ring.startup_events.len(), CLEANUP_EVENT_BUDGET);
        assert_eq!(ring.startup_events_dropped, 4);
        assert!(!ring.is_active());
        let mut trace = SessionState::default();
        ring.poll_cleanup(&mut trace).unwrap();
        assert_eq!(trace[0]["kind"], "ring_startup_observations_dropped");
        assert_eq!(trace[0]["count"], 4);
        assert!(ring.startup_events.is_empty());
        fs::remove_dir(ring_directory).unwrap();
    }

    #[test]
    fn optional_startup_budget_counts_owned_exit_exactly_once() {
        use crate::process::ring_startup_tests::await_exit;
        // Exercise no pressure, exactly one discarded lifecycle receipt, and
        // three injected overflows followed by two real lifecycle receipts.
        for injected in [0, CLEANUP_EVENT_BUDGET - 1, CLEANUP_EVENT_BUDGET + 3] {
            let directory = SessionDirectory::create().unwrap();
            let (worker, mut peer, input) = Worker::cooperative_fixture(&directory.path, "ring");
            let mut ring = OptionalRing::from(Some(worker));
            ring.disable("owned child unavailable during startup");
            assert!(matches!(
                peer.control.receive().unwrap(),
                Some(Control::Stop)
            ));
            drop(input);
            let status = await_exit(ring.iter_mut().next().unwrap());
            assert!(status.success());
            for index in 0..injected {
                ring.defer_startup(json!({"kind":"test_ring_observation", "index":index}));
            }
            // Production startup_tick emits ring_degraded AND the observed
            // ring_degraded_exit, even if its fixed optional buffer is full.
            ring.startup_tick(None);
            let expected_dropped = (injected + 2).saturating_sub(CLEANUP_EVENT_BUDGET);
            assert_eq!(
                ring.startup_events_dropped, expected_dropped as u64,
                "injected={injected}"
            );
            assert_eq!(
                ring.startup_events.len(),
                (injected + 2).min(CLEANUP_EVENT_BUDGET)
            );
            let state = ring.degraded.as_ref().unwrap();
            assert!(state.exit_observed);
            assert!(!state.notice_pending);
            assert!(
                !state.kill_requested,
                "an observed exit must not be signaled"
            );
            assert!(!ring.is_active());
            assert!(!ring.blank_confirmed(), "exit is not a black-write receipt");
            let retained_kinds: Vec<_> = ring
                .startup_events
                .iter()
                .map(|event| event["kind"].as_str().unwrap())
                .collect();
            let mut expected_kinds = vec!["test_ring_observation"; injected];
            expected_kinds.extend(["ring_degraded", "ring_degraded_exit"]);
            expected_kinds.truncate(CLEANUP_EVENT_BUDGET);
            assert_eq!(retained_kinds, expected_kinds);
            let mut trace = SessionState::default();
            ring.poll_cleanup(&mut trace).unwrap();
            if expected_dropped != 0 {
                assert_eq!(trace[0]["kind"], "ring_startup_observations_dropped");
                assert_eq!(trace[0]["count"], expected_dropped);
            }
            assert_eq!(
                trace.len(),
                (injected + 2).min(CLEANUP_EVENT_BUDGET) + usize::from(expected_dropped != 0)
            );
            assert!(ring.startup_events.is_empty());
            assert_eq!(ring.startup_events_dropped, 0);
            let flushed = trace.len();
            ring.startup_tick(None);
            ring.poll_cleanup(&mut trace).unwrap();
            assert_eq!(trace.len(), flushed, "old lifecycle events must not repeat");
        }
    }

    /// #1620: past its third restart a faulted ring stays off, says so once,
    /// and the voice's health is told; a restart that cannot start counts.
    #[test]
    fn a_ring_past_its_restarts_stays_off_and_says_so_once() {
        let directory = SessionDirectory::create().unwrap();
        let restart = |begun| Restart {
            executable: directory.path.join("absent"),
            directory: directory.path.clone(),
            parent: new_boot().unwrap(),
            extra: Vec::new(),
            begun,
            pending: None,
            off_said: false,
        };
        // The third restart: its startup fails, and that is the last.
        let (worker, _peer, input) = Worker::cooperative_fixture(&directory.path, "ring");
        let mut ring = OptionalRing::from(Some(worker));
        ring.restart = Some(restart(RING_RESTARTS - 1));
        drop(input);
        ring.disable("ring output fault: SPI write returned after 15000 us; budget is 10000 us, late for 2000 ms");
        assert!(!ring.off_for_good());
        let mut trace = SessionState::default();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !ring.degraded.as_ref().unwrap().exit_observed {
            assert!(std::time::Instant::now() < deadline, "fixture never exited");
            ring.poll_cleanup(&mut trace).unwrap();
            std::thread::sleep(Duration::from_millis(2));
        }
        let later = || u64::MAX / 2;
        let tick = |ring: &mut OptionalRing, trace: &mut SessionState| {
            ring.restart_tick_with_clock(|event| record(trace, event), later)
                .unwrap()
        };
        assert!(!tick(&mut ring, &mut trace));
        assert_eq!(ring.restart.as_ref().unwrap().begun, RING_RESTARTS);
        while ring.restart.as_ref().unwrap().pending.is_some() {
            assert!(!tick(&mut ring, &mut trace));
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(ring.off_for_good());
        assert!(!ring.is_active());
        // A fourth fault starts nothing, and is said once.
        for _ in 0..3 {
            assert!(!tick(&mut ring, &mut trace));
        }
        assert!(ring.restart.as_ref().unwrap().pending.is_none());
        let kinds = |kind: &str| trace.iter().filter(|e| e["kind"] == kind).count();
        assert_eq!(kinds("ring_restart"), 1);
        assert_eq!(kinds("ring_off"), 1);
        assert!(ring.off_for_good());
        // A fourth fault of a live ring: off for good, and its observed exit
        // is dated before a later stop (#1961).
        let own = SessionDirectory::create().unwrap();
        let (worker, _peer, input) = Worker::cooperative_fixture(&own.path, "ring");
        let mut fourth = OptionalRing::from(Some(worker));
        fourth.restart = Some(restart(RING_RESTARTS));
        drop(input);
        fourth.disable("ring output fault: SPI write returned after 15000 us; budget is 10000 us, late for 2000 ms");
        while !fourth.exit_observed() {
            assert!(std::time::Instant::now() < deadline, "fixture never exited");
            fourth.poll_cleanup(&mut trace).unwrap();
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(!tick(&mut fourth, &mut trace));
        assert!(fourth.off_for_good() && fourth.exit_observed());
        // A ring that never started is off for good too, with no restart.
        let mut never = OptionalRing::from(None);
        never.latch_disabled("ring startup failed: no device");
        assert!(never.off_for_good());
    }

    #[test]
    fn hung_ring_gets_one_non_waiting_signal_without_a_cleanup_success_claim() {
        let directory = SessionDirectory::create().unwrap();
        let (worker, mut peer) = Worker::sleeping_fixture(&directory.path, "ring");
        let mut ring = OptionalRing::from(Some(worker));
        ring.disable("test hung SPI");
        assert!(matches!(
            peer.control.receive().unwrap(),
            Some(Control::Stop)
        ));
        let at = ring.stop_requested_at(0);
        let mut trace = SessionState::default();
        ring.poll_cleanup_with_clock(|event| record(&mut trace, event), || at + STOP_GRACE_US - 1)
            .unwrap();
        assert!(!ring.degraded.as_ref().unwrap().kill_requested);
        ring.poll_cleanup_with_clock(|event| record(&mut trace, event), || at + STOP_GRACE_US)
            .unwrap();
        assert!(ring.degraded.as_ref().unwrap().kill_requested);
        assert!(!ring.degraded.as_ref().unwrap().exit_observed);
        assert!(!ring.blank_confirmed());
        assert!(ring.cleanup_error().is_some());
        ring.poll_cleanup_with_clock(|event| record(&mut trace, event), || at + STOP_GRACE_US + 1)
            .unwrap();
        assert_eq!(
            trace
                .iter()
                .filter(|e| e["kind"] == "ring_degraded_kill_requested")
                .count(),
            1
        );
        assert!(ring.iter_mut().next().unwrap().shutdown().is_err());
        assert!(
            ring.iter_mut()
                .next()
                .unwrap()
                .try_exit()
                .unwrap()
                .is_some()
        );
    }
}
