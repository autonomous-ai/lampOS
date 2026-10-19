//! Disable failed presentation immediately; observe cleanup without waiting on it.
use super::*;
use crate::process::RingStartup;

const STOP_GRACE_US: u64 = 300_000;
const CLEANUP_EVENT_BUDGET: usize = 16;

pub(super) struct OptionalRing {
    worker: Option<Worker>,
    degraded: Option<Degraded>,
    startup_events: Vec<Value>,
    startup_events_dropped: u64,
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
            RingStartup::Ready(worker) => Ok(Self::from(Some(*worker))),
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
