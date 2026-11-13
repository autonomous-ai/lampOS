//! The only real-device entry. Explicit provisioning, existing exclusive owner.
use super::{
    SystemClock,
    channel::Channels,
    config::Config,
    wire::{self, Control, Event},
    worker::{self, Backend, Link, Worker},
};
use crate::{
    control::{self, Clock, MotionId, Placement, Tick},
    linux::{InspectionConfig, motion::LinuxMotor},
    stop_signal::StopSignal,
    units::NormalizedPosition,
};
use lamp_turn_owner::{
    BootId, BoundaryGuard, Error as AuthorityError, OutputPermit, Permission, Snapshot,
};
use std::{io, time::Duration};

impl<C: Clock> Backend for LinuxMotor<C> {
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
        selection: crate::command::JointSelection,
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
            self.inhibit()
        } else {
            self.control_lost()
        }
    }
    fn stopped(&self) -> Option<control::StopReport> {
        self.stop_report()
    }
    fn finish(self) -> wire::Cleanup {
        let r = self.shutdown();
        wire::Cleanup {
            serial_closed: Some(
                r.serial.tty_close_attempted
                    && r.serial
                        .errors
                        .iter()
                        .all(|error| error.operation != "close_tty"),
            ),
            settings_restored: Some(
                r.serial.prior_settings_restore_attempted
                    && r.serial.restored_settings.is_some()
                    && r.serial.errors.is_empty(),
            ),
            error: (!r.serial.errors.is_empty())
                .then(|| wire::bounded(format!("{:?}", r.serial.errors))),
        }
    }
}
struct NativeLink {
    channels: Channels,
    signals: StopSignal,
    /// Without PR_SET_PDEATHSIG (Linux only) the parent's death is noticed
    /// here, once a control poll: the parent pid changes when it is gone.
    #[cfg(not(target_os = "linux"))]
    parent_pid: u32,
}
impl Link for NativeLink {
    fn control(
        &mut self,
        now: &mut dyn FnMut() -> io::Result<lamp_turn_owner::MonoTime>,
    ) -> io::Result<Option<Control>> {
        if self.signals.requested()? {
            return Ok(Some(Control::Stop {}));
        }
        #[cfg(not(target_os = "linux"))]
        if rustix::process::getppid().map(|p| p.as_raw_nonzero().get() as u32)
            != Some(self.parent_pid)
        {
            return Ok(Some(Control::Stop {}));
        }
        self.channels.control(now)
    }
    fn command(
        &mut self,
        now: &mut dyn FnMut() -> io::Result<lamp_turn_owner::MonoTime>,
    ) -> io::Result<Option<wire::Command>> {
        self.channels.command(now)
    }
    fn publish(&mut self, e: &Event, n: lamp_turn_owner::MonoTime) -> io::Result<()> {
        self.channels.publish(e, n)
    }
}
/// Called by a single-threaded child only. SIGTERM/SIGINT are converted to local
/// Stop; SIGKILL and a wedged driver require the parent's truthful forced report.
pub fn run(
    config: Config,
    mut channels: Channels,
    parent: BootId,
    worker_boot: BootId,
    parent_pid: u32,
) -> io::Result<()> {
    if rustix::process::getppid().map(|p| p.as_raw_nonzero().get() as u32) != Some(parent_pid)
        || parent_pid <= 1
    {
        return Err(io::Error::other(
            "motor parent identity changed before startup",
        ));
    }
    #[cfg(target_os = "linux")]
    rustix::process::set_parent_process_death_signal(Some(rustix::process::Signal::KILL))?;
    if rustix::process::getppid().map(|p| p.as_raw_nonzero().get() as u32) != Some(parent_pid) {
        return Err(io::Error::other(
            "motor parent died while arming parent-death signal",
        ));
    }
    let valid = config.validate()?;
    let clock = SystemClock;
    let started = clock.now();
    let mut link = NativeLink {
        signals: StopSignal::new()?,
        #[cfg(not(target_os = "linux"))]
        parent_pid,
        channels: {
            while !channels.connect_step()? {
                if clock.now().as_micros().saturating_sub(started.as_micros()) >= 3_000_000 {
                    return Err(io::Error::other("motor connection startup timed out"));
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            channels
        },
    };
    let mut guard = BoundaryGuard::new(parent, clock.now());
    let waiting = clock.now();
    // Opening is gated too: no serial side effects without fresh independent
    // physical privacy authority. Setup never consumes a data/motion command.
    loop {
        if opening_controls(&mut link, &mut guard)? {
            return Err(io::Error::other("motor stopped before open"));
        }
        if guard.state().is_some() {
            break;
        }
        if clock.now().as_micros() - waiting.as_micros() >= worker::INITIAL_AUTHORITY_US {
            return Err(io::Error::other("motor initial authority missing"));
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let inspection = InspectionConfig::new(
        config.device.clone(),
        valid.unit.clone(),
        config.lock_directory.clone(),
    )?;
    let d = config.expected_device;
    let opened = LinuxMotor::open(
        &inspection,
        crate::linux::DeviceIdentity {
            filesystem_device: d.filesystem_device,
            inode: d.inode,
            character_device: d.character_device,
            major: d.major,
            minor: d.minor,
        },
        clock,
        parent,
        worker_boot,
        valid.calibration,
        valid.timing,
        || opening_controls(&mut link, &mut guard),
    );
    let (mut motor, _open) = match opened {
        Ok(v) => v,
        Err(e) => {
            let reason = wire::bounded(format!("{e}; cleanup {:?}", e.cleanup.errors));
            let _ = link.publish(
                &Event::Fault {
                    at_us: clock.now().as_micros(),
                    reason: reason.clone(),
                },
                clock.now(),
            );
            return Err(io::Error::other(reason));
        }
    };
    // Qualification has original absolute monotonic bounds; opening never
    // restamps/extends it. A rejected claim still closes the exclusive resource.
    if let Err(e) = motor.qualify(valid.qualification) {
        let cleanup = motor.shutdown();
        return Err(io::Error::other(format!(
            "qualification: {e}; cleanup {:?}",
            cleanup.serial.errors
        )));
    }
    let snapshot = guard.state().expect("opening required authority");
    if let Err(e) = motor.install(snapshot) {
        let cleanup = motor.shutdown();
        return Err(io::Error::other(format!(
            "opening authority: {e}; cleanup {:?}",
            cleanup.serial.errors
        )));
    }
    let mut engine = Worker::new(
        motor,
        clock,
        parent,
        worker_boot,
        config.placement_source,
        config.maximum_runtime_us,
    )?;
    // Reuse the actual latest opening authority, not a freshly minted lease.
    engine.initial_authority(snapshot)?;
    worker::run(engine, &mut link)
}
fn opening_controls(link: &mut impl Link, guard: &mut BoundaryGuard) -> io::Result<bool> {
    let clock = SystemClock;
    let mut drained = false;
    for _ in 0..worker::CONTROL_BUDGET {
        let Some(c) = link.control(&mut || Ok(clock.now()))? else {
            drained = true;
            break;
        };
        let n = clock.now();
        match c {
            Control::Stop {} => return Ok(true),
            Control::Cancel { motion } => link.publish(
                &Event::CancelRejected {
                    motion,
                    at_us: n.as_micros(),
                },
                n,
            )?,
            Control::Authority { snapshot } => {
                if guard
                    .state()
                    .is_some_and(|s| s.revision() == snapshot.revision() && s != snapshot)
                {
                    return Err(io::Error::other("conflicting opening authority"));
                }
                match guard.install(n, snapshot) {
                    Ok(()) => {}
                    Err(AuthorityError::StaleSnapshot) => {}
                    Err(e) => return Err(io::Error::other(e)),
                }
            }
        }
    }
    if !drained {
        return Err(io::Error::other("opening control flood"));
    }
    if let Some(s) = guard.state()
        && (clock.now() >= s.expires_at()
            || s.microphone_permission() != Permission::Allowed
            || s.camera_permission() != Permission::Allowed)
    {
        return Err(io::Error::other("opening privacy/authority unavailable"));
    }
    Ok(false)
}
