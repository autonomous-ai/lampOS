//! Nonblocking worker steps over the actual safety core. No background bus work.
use super::{
    channel::Channels,
    config::invalid,
    wire::{self, Command, Control, Event, Motion},
};
use crate::{
    command::JointSelection,
    control::{self, BusPort, Clock, MotionId, MotorControl, Placement, Tick},
    units::NormalizedPosition,
};
use lamp_turn_owner::{
    BootId, BoundaryGuard, Error as AuthorityError, MonoTime, OutputPermit, Permission, Snapshot,
};
use std::{io, time::Duration};

pub const TICK_US: u64 = 2_000;
pub const CONTROL_BUDGET: usize = 16;
pub const INITIAL_AUTHORITY_US: u64 = 250_000;
pub const PROGRESS_US: u64 = 10_000;
/// Adapter methods perform no waits/retries. Only finish may restore/close the
/// owned serial resource; the parent enforces a separate teardown deadline.
pub trait Backend {
    fn install(&mut self, state: Snapshot) -> Result<(), control::Error>;
    fn inspect(&mut self) -> Result<(), control::Error>;
    fn start(
        &mut self,
        permit: OutputPermit,
        placement: Placement,
        target: [NormalizedPosition; 5],
        duration_us: u64,
    ) -> Result<MotionId, control::Error>;
    fn start_selected(
        &mut self,
        permit: OutputPermit,
        placement: Placement,
        target: [NormalizedPosition; 5],
        duration_us: u64,
        selection: JointSelection,
    ) -> Result<MotionId, control::Error> {
        if selection != JointSelection::ALL {
            return Err(control::Error::Configuration);
        }
        self.start(permit, placement, target, duration_us)
    }
    fn prepare_profile(
        &mut self,
        _permit: OutputPermit,
        _placement: Placement,
        _speed: u16,
        _acceleration: u8,
    ) -> Result<MotionId, control::Error> {
        Err(control::Error::ProfilePreparation)
    }
    fn tick(&mut self) -> Result<Tick, control::Error>;
    fn readback(&mut self) -> Result<control::Readback, control::Error> {
        Err(control::Error::NotInspected)
    }
    fn cancel(&mut self, id: MotionId) -> Result<control::StopReport, control::Error>;
    fn inhibit(&mut self, normal: bool) -> control::StopReport;
    fn stopped(&self) -> Option<control::StopReport>;
    fn finish(self) -> wire::Cleanup;
}
impl<P: BusPort, C: Clock> Backend for MotorControl<P, C> {
    fn install(&mut self, s: Snapshot) -> Result<(), control::Error> {
        self.install(s)
    }
    fn inspect(&mut self) -> Result<(), control::Error> {
        self.inspect()
    }
    fn start(
        &mut self,
        p: OutputPermit,
        o: Placement,
        t: [NormalizedPosition; 5],
        d: u64,
    ) -> Result<MotionId, control::Error> {
        self.start_segment(p, o, t, d)
    }
    fn start_selected(
        &mut self,
        p: OutputPermit,
        o: Placement,
        t: [NormalizedPosition; 5],
        d: u64,
        selection: JointSelection,
    ) -> Result<MotionId, control::Error> {
        self.start_selected_segment(p, o, t, d, selection)
    }
    fn readback(&mut self) -> Result<control::Readback, control::Error> {
        self.readback()
    }
    fn prepare_profile(
        &mut self,
        permit: OutputPermit,
        placement: Placement,
        speed: u16,
        acceleration: u8,
    ) -> Result<MotionId, control::Error> {
        self.prepare_head_profile(permit, placement, speed, acceleration)
    }
    fn tick(&mut self) -> Result<Tick, control::Error> {
        self.tick()
    }
    fn cancel(&mut self, m: MotionId) -> Result<control::StopReport, control::Error> {
        self.cancel(m)
    }
    fn inhibit(&mut self, normal: bool) -> control::StopReport {
        if normal {
            self.shutdown()
        } else {
            self.control_lost()
        }
    }
    fn stopped(&self) -> Option<control::StopReport> {
        self.stop_report()
    }
    fn finish(mut self) -> wire::Cleanup {
        self.shutdown();
        wire::Cleanup::default()
    }
}
pub trait Link {
    /// `now` is read once a packet has arrived, never before the receive.
    fn control(
        &mut self,
        now: &mut dyn FnMut() -> io::Result<MonoTime>,
    ) -> io::Result<Option<Control>>;
    fn command(
        &mut self,
        now: &mut dyn FnMut() -> io::Result<MonoTime>,
    ) -> io::Result<Option<Command>>;
    fn publish(&mut self, event: &Event, now: MonoTime) -> io::Result<()>;
}
impl Link for Channels {
    fn control(
        &mut self,
        now: &mut dyn FnMut() -> io::Result<MonoTime>,
    ) -> io::Result<Option<Control>> {
        self.control.receive_with(now)
    }
    fn command(
        &mut self,
        now: &mut dyn FnMut() -> io::Result<MonoTime>,
    ) -> io::Result<Option<Command>> {
        self.data.receive_with(now)
    }
    fn publish(&mut self, e: &Event, n: MonoTime) -> io::Result<()> {
        self.control.send(e, n)
    }
}
/// Every nonblocking step handles control before data and again before bus work.
/// The only retained request is the original (unrestamped) command deferred by a
/// full control slice. Kernel queues are finite; feedback backpressure is fatal.
pub struct Worker<B: Backend, C: Clock> {
    backend: B,
    clock: C,
    guard: BoundaryGuard,
    worker_boot: BootId,
    placement_source: BootId,
    started: u64,
    last_now: u64,
    maximum_runtime_us: u64,
    last_progress: u64,
    ready: bool,
    pending: Option<Command>,
    last_request: u64,
    inspection: Option<u64>,
    active: Option<MotionId>,
    last_frame: u64,
    failed: Option<String>,
    inhibited_sent: bool,
    normal_stop: bool,
    privacy_revision: Option<u64>,
}
impl<B: Backend, C: Clock> Worker<B, C> {
    pub fn new(
        backend: B,
        clock: C,
        parent: BootId,
        worker: BootId,
        placement_source: BootId,
        maximum_runtime_us: u64,
    ) -> io::Result<Self> {
        if !(1_000..=630_000_000).contains(&maximum_runtime_us) {
            return Err(invalid("invalid worker lifetime"));
        }
        let n = clock.now();
        Ok(Self {
            backend,
            clock,
            guard: BoundaryGuard::new(parent, n),
            worker_boot: worker,
            placement_source,
            started: n.as_micros(),
            last_now: n.as_micros(),
            maximum_runtime_us,
            last_progress: n.as_micros(),
            ready: false,
            pending: None,
            last_request: 0,
            inspection: None,
            active: None,
            last_frame: 0,
            failed: None,
            inhibited_sent: false,
            normal_stop: false,
            privacy_revision: None,
        })
    }
    /// Native open already consumed this exact priority snapshot. It is not renewed.
    #[cfg(unix)]
    pub(crate) fn initial_authority(&mut self, snapshot: Snapshot) -> io::Result<()> {
        let n = self.now()?;
        self.guard.install(n, snapshot).map_err(invalid)?;
        self.authority()
    }
    fn now(&mut self) -> io::Result<MonoTime> {
        let n = self.clock.now();
        if n.as_micros() < self.last_now {
            return Err(invalid("motor worker clock regressed"));
        }
        self.last_now = n.as_micros();
        Ok(n)
    }
    fn publish(&mut self, link: &mut impl Link, event: Event) -> io::Result<()> {
        let n = self.now()?;
        link.publish(&event, n)
    }
    fn authority(&mut self) -> io::Result<()> {
        let n = self.now()?;
        let Some(s) = self.guard.state() else {
            return if n.as_micros() - self.started >= INITIAL_AUTHORITY_US {
                Err(invalid("initial motor authority timeout"))
            } else {
                Ok(())
            };
        };
        if n >= s.expires_at()
            || s.microphone_permission() != Permission::Allowed
            || s.camera_permission() != Permission::Allowed
        {
            return Err(invalid("motor privacy or authority expired"));
        }
        Ok(())
    }
    fn controls(&mut self, link: &mut impl Link, budget: &mut usize) -> io::Result<bool> {
        while *budget > 0 {
            let Some(c) = link.control(&mut || self.now())? else {
                return Ok(false);
            };
            let n = self.now()?;
            *budget -= 1;
            match c {
                Control::Stop {} => {
                    self.normal_stop = true;
                    self.backend.inhibit(true);
                    return Ok(true);
                }
                Control::Cancel { motion } => {
                    if let Some(active) = self.active.filter(|m| Motion::from(*m) == motion) {
                        self.backend.cancel(active).map_err(invalid)?;
                        self.normal_stop = true;
                        return Ok(true);
                    }
                    self.publish(
                        link,
                        Event::CancelRejected {
                            motion,
                            at_us: n.as_micros(),
                        },
                    )?;
                }
                Control::Authority { snapshot } => {
                    if self
                        .guard
                        .state()
                        .is_some_and(|s| s.revision() == snapshot.revision() && s != snapshot)
                    {
                        return Err(invalid("conflicting motor authority revision"));
                    }
                    match self.guard.install(n, snapshot) {
                        Ok(()) => {}
                        Err(AuthorityError::StaleSnapshot) => continue,
                        Err(AuthorityError::ExpiredState)
                            if self
                                .guard
                                .state()
                                .is_some_and(|s| snapshot.revision() <= s.revision()) =>
                        {
                            continue;
                        }
                        Err(e) => return Err(invalid(e)),
                    }
                    if snapshot.microphone_permission() == Permission::Denied
                        || snapshot.camera_permission() == Permission::Denied
                    {
                        self.privacy_revision = Some(snapshot.revision());
                    }
                    self.authority()?;
                    self.backend.install(snapshot).map_err(invalid)?;
                }
            }
        }
        Ok(false)
    }
    fn reject(
        &mut self,
        link: &mut impl Link,
        request: u64,
        reason: impl std::fmt::Display,
    ) -> io::Result<()> {
        let n = self.now()?;
        self.publish(
            link,
            Event::Rejected {
                request,
                at_us: n.as_micros(),
                reason: wire::bounded(reason),
            },
        )
    }
    fn command_step(&mut self, link: &mut impl Link, command: Command) -> io::Result<()> {
        let n = self.now()?;
        let (request, requested) = command.identity();
        if request == 0 || request <= self.last_request {
            return self.reject(link, request, "stale request");
        }
        self.last_request = request;
        if n.as_micros()
            .checked_sub(requested)
            .is_none_or(|age| age > super::channel::MAX_AGE_US)
        {
            return self.reject(link, request, "request expired or future");
        }
        if self.inspection.is_some() || self.active.is_some() {
            return self.reject(link, request, "worker busy; no motion queue");
        }
        match command {
            Command::PrepareHeadProfile {
                permit,
                placement,
                speed_raw,
                acceleration_raw,
                ..
            } => {
                if placement.source_boot != self.placement_source
                    || placement.frame_sequence <= self.last_frame
                {
                    return self.reject(link, request, "wrong or reused placement observation");
                }
                match self.backend.prepare_profile(
                    permit,
                    placement.into(),
                    speed_raw,
                    acceleration_raw,
                ) {
                    Ok(m) => {
                        if m.worker_boot() != self.worker_boot {
                            return Err(invalid("backend returned wrong worker identity"));
                        }
                        self.active = Some(m);
                        self.last_frame = placement.frame_sequence;
                        self.publish(
                            link,
                            Event::ProfileStarted {
                                request,
                                motion: m.into(),
                                at_us: n.as_micros(),
                            },
                        )?;
                    }
                    Err(e) => self.reject(link, request, e)?,
                }
            }
            Command::Inspect { .. } => match self.backend.inspect() {
                Ok(()) => self.inspection = Some(request),
                Err(e) => self.reject(link, request, e)?,
            },
            command @ (Command::Segment { .. } | Command::SelectedSegment { .. }) => {
                let (permit, placement, target, duration_us, selection) = match command {
                    Command::Segment {
                        permit,
                        placement,
                        target,
                        duration_us,
                        ..
                    } => (permit, placement, target, duration_us, JointSelection::ALL),
                    Command::SelectedSegment {
                        permit,
                        placement,
                        target,
                        duration_us,
                        selection,
                        ..
                    } => (permit, placement, target, duration_us, selection),
                    Command::Inspect { .. } | Command::PrepareHeadProfile { .. } => {
                        unreachable!("matched segment")
                    }
                };
                if placement.source_boot != self.placement_source
                    || placement.frame_sequence <= self.last_frame
                {
                    return self.reject(link, request, "wrong or reused placement observation");
                }
                let mut values = [NormalizedPosition::new(0.0).expect("zero"); 5];
                for (value, raw) in values.iter_mut().zip(target) {
                    match NormalizedPosition::new(raw) {
                        Ok(n) => *value = n,
                        Err(e) => return self.reject(link, request, e),
                    }
                }
                match self.backend.start_selected(
                    permit,
                    placement.into(),
                    values,
                    duration_us,
                    selection,
                ) {
                    Ok(m) => {
                        if m.worker_boot() != self.worker_boot {
                            return Err(invalid("backend returned wrong worker identity"));
                        }
                        self.active = Some(m);
                        self.last_frame = placement.frame_sequence;
                        self.publish(
                            link,
                            Event::Started {
                                request,
                                motion: m.into(),
                                at_us: n.as_micros(),
                            },
                        )?
                    }
                    Err(e) => self.reject(link, request, e)?,
                }
            }
        }
        if self.backend.stopped().is_some() {
            return Err(invalid("motor core inhibited during command"));
        }
        Ok(())
    }
    fn inner(&mut self, link: &mut impl Link) -> io::Result<bool> {
        let n = self.now()?;
        if n.as_micros() - self.started >= self.maximum_runtime_us {
            return Err(invalid("finite motor worker duration expired"));
        }
        let mut budget = CONTROL_BUDGET;
        if self.controls(link, &mut budget)? {
            return Ok(true);
        }
        self.authority()?;
        if self.guard.state().is_none() {
            return Ok(false);
        }
        if !self.ready {
            // Stamped now, after the controls that installed the authority,
            // not at the step's start: a backlog read on a slow host would
            // date Ready old enough for the Supervisor to time it out.
            let at_us = self.now()?.as_micros();
            self.publish(link, Event::Ready { at_us })?;
            self.ready = true;
        }
        if budget > 0 && self.pending.is_none() {
            self.pending = link.command(&mut || self.now())?;
        }
        if self.controls(link, &mut budget)? {
            return Ok(true);
        }
        self.authority()?;
        if budget > 0 {
            if let Some(command) = self.pending.take() {
                self.command_step(link, command)?;
            }
            // Command admission does not itself write. Recheck priority control
            // once more immediately before the core's one-read/one-write tick.
            if self.controls(link, &mut budget)? {
                return Ok(true);
            }
            self.authority()?;
            if budget > 0 {
                match self.backend.tick().map_err(invalid)? {
                    Tick::Idle | Tick::Progress => {}
                    Tick::ProfilePrepared {
                        motion,
                        at,
                        progress,
                    } => {
                        if self.active != Some(motion) {
                            return Err(invalid("wrong profile preparation receipt"));
                        }
                        self.active = None;
                        let readback = self.backend.readback().map_err(invalid)?;
                        self.publish(
                            link,
                            Event::ProfilePrepared {
                                motion: motion.into(),
                                at_us: at.as_micros(),
                                progress,
                                readback,
                            },
                        )?;
                    }
                    Tick::Inspected => {
                        let request = self
                            .inspection
                            .take()
                            .ok_or_else(|| invalid("unsolicited inspection receipt"))?;
                        let readback = self.backend.readback().map_err(invalid)?;
                        let at_us = self.now()?.as_micros();
                        self.publish(
                            link,
                            Event::Inspected {
                                request,
                                at_us,
                                readback,
                            },
                        )?
                    }
                    Tick::PositionAccepted {
                        selection,
                        motion,
                        at,
                        counts,
                    } => {
                        if self.active != Some(motion) {
                            return Err(invalid("wrong active position receipt"));
                        }
                        self.publish(
                            link,
                            Event::HostAccepted {
                                selection,
                                motion: motion.into(),
                                counts: counts.map(|c| c.get()),
                                at_us: at.as_micros(),
                            },
                        )?;
                    }
                    Tick::Reached { motion, at } => {
                        if self.active != Some(motion) {
                            return Err(invalid("wrong endpoint receipt"));
                        }
                        self.active = None;
                        let readback = self.backend.readback().map_err(invalid)?;
                        self.publish(
                            link,
                            Event::ReadbackReached {
                                motion: motion.into(),
                                at_us: at.as_micros(),
                                readback,
                            },
                        )?;
                    }
                }
            }
        }
        let n = self.now()?;
        if n.as_micros() - self.last_progress >= PROGRESS_US {
            self.publish(
                link,
                Event::Progress {
                    at_us: n.as_micros(),
                },
            )?;
            self.last_progress = n.as_micros();
        }
        Ok(false)
    }
    /// An error is latched and locally inhibited before return. Caller must
    /// still call finish exactly once to report and release the owned resource.
    pub fn poll(&mut self, link: &mut impl Link) -> io::Result<bool> {
        if self.backend.stopped().is_some() {
            return Ok(true);
        }
        match self.inner(link) {
            Ok(done) => Ok(done),
            Err(e) => {
                self.pending = None;
                self.backend.inhibit(false);
                self.failed.get_or_insert_with(|| wire::bounded(&e));
                Err(e)
            }
        }
    }
    pub fn finish(mut self, link: &mut impl Link) -> io::Result<()> {
        self.pending = None;
        let report = self.backend.inhibit(self.normal_stop);
        let mut evidence = wire::StopEvidence::from(report);
        evidence.privacy_revision = self.privacy_revision;
        let mut failure = self.failed.take();
        if report.discard_error.is_some() {
            failure.get_or_insert_with(|| "motor discard failed".into());
        }
        if !self.inhibited_sent {
            if let Err(e) = self.publish(
                link,
                Event::Inhibited {
                    at_us: self.clock.now().as_micros(),
                    motion: self.active.map(Into::into),
                    report: evidence,
                },
            ) {
                failure.get_or_insert_with(|| wire::bounded(e));
            }
            self.inhibited_sent = true;
        }
        let clock = self.clock;
        let cleanup = self.backend.finish();
        if cleanup.error.is_some()
            || cleanup.serial_closed == Some(false)
            || cleanup.settings_restored == Some(false)
        {
            failure.get_or_insert_with(|| "motor serial cleanup failed".into());
        }
        if let Err(e) = link.publish(
            &Event::Stopped {
                at_us: clock.now().as_micros(),
                cleanup,
            },
            clock.now(),
        ) {
            failure.get_or_insert_with(|| wire::bounded(e));
        }
        if let Some(reason) = failure {
            let _ = link.publish(
                &Event::Fault {
                    at_us: clock.now().as_micros(),
                    reason: reason.clone(),
                },
                clock.now(),
            );
            Err(io::Error::other(reason))
        } else {
            Ok(())
        }
    }
}
/// Same loop used by Linux and inert process tests. Sleep is only between bounded
/// steps; no process-loop retry, automatic reopen or command replay occurs.
pub fn run<B: Backend, C: Clock>(mut worker: Worker<B, C>, link: &mut impl Link) -> io::Result<()> {
    while let Ok(false) = worker.poll(link) {
        std::thread::sleep(Duration::from_micros(TICK_US));
    }
    worker.finish(link)
}
