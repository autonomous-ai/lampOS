//! Single-owner, finite position control over a nonblocking serial seam.
//!
//! This owns request/response framing, position writes and explicit guarded
//! ID5 profile preparation. It does not open
//! a tty, authenticate qualification evidence, run a thread, or release torque.
//! A supervised worker must own the exclusive port and service priority control
//! before every tick. Its clock must use the controller's OS monotonic domain.

use crate::{
    JOINT_COUNT, Joint, STS3215_MODEL_NUMBER,
    calibration::{CalibrationError, UnitCalibration, UnitId},
    command::{JointSelection, POSITION_FRAME_BYTES, SelectedPositionFrame},
    protocol::{ReadOnlyRequest, ReadWindow, ReplyError, ReplyTracker, StatusParser},
    readback::{HomingModeRegisters, PositionLimitRegisters, RawTelemetry},
    trajectory::{JointEnvelope, RawProfile, Segment, TrajectoryError},
    units::{EncoderCounts, NormalizedPosition},
};
use lamp_turn_owner::{
    BootId, BoundaryGuard, CameraGrant, MonoTime, OutputKind, OutputPermit, Snapshot, TurnOwner,
};
use std::{fmt, io};

mod profile;
use profile::Preparing;
pub use profile::ProfileProgress;

const READ_BYTES: usize = 128;
const MAX_OPERATIONS: u16 = 512;
const QUIET_US: u64 = 20_000;

/// One exclusively owned nonblocking bus. Calls must not wait or retry. A
/// successful write reports only bytes accepted by the host. Discard is one
/// best-effort host-output flush, never a torque command or proof of servo stop.
/// No core method exposes this port or accepts arbitrary instruction bytes.
pub trait BusPort {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize>;
    fn write(&mut self, input: &[u8]) -> io::Result<usize>;
    fn discard_output(&mut self) -> io::Result<()>;
}

pub trait Clock {
    fn now(&self) -> MonoTime;
}

/// Software work/freshness bounds, all explicitly supplied. These are not
/// sensor acquisition rates, collision limits, or hard syscall deadlines.
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    transaction_us: u64,
    io_us: u64,
    readback_age_us: u64,
    placement_age_us: u64,
    interval_us: u64,
}

impl Timing {
    pub fn new(
        transaction_us: u64,
        io_us: u64,
        readback_age_us: u64,
        placement_age_us: u64,
        interval_us: u64,
    ) -> Result<Self, Error> {
        if !(1..=200_000).contains(&transaction_us)
            || !(1..=20_000).contains(&io_us)
            || io_us > transaction_us
            || !(transaction_us..=250_000).contains(&readback_age_us)
            || !(1..=1_000_000).contains(&placement_age_us)
            || !(1_000..=20_000).contains(&interval_us)
        {
            return Err(Error::Configuration);
        }
        Ok(Self {
            transaction_us,
            io_us,
            readback_age_us,
            placement_age_us,
            interval_us,
        })
    }
}

/// A trusted provisioner's claim covering this exact calibration and envelope.
/// A nonzero record ID and matching data are necessary, not proof that physical
/// qualification happened. There is deliberately no default or built-in unit.
#[derive(Clone, Debug)]
pub struct QualifiedEnvelope {
    record: [u8; 32],
    calibration: UnitCalibration,
    limits: [JointEnvelope; JOINT_COUNT],
    valid_from: MonoTime,
    expires_at: MonoTime,
    allowed: JointSelection,
    prepare_from: Option<[RawProfile; JOINT_COUNT]>,
}

impl QualifiedEnvelope {
    pub fn new(
        record: [u8; 32],
        calibration: UnitCalibration,
        limits: [JointEnvelope; JOINT_COUNT],
        valid_from: MonoTime,
        expires_at: MonoTime,
    ) -> Result<Self, Error> {
        Self::selected(
            record,
            calibration,
            limits,
            valid_from,
            expires_at,
            JointSelection::ALL,
            None,
        )
    }
    /// Selected-only policy. Unselected profiles are exact observed bindings,
    /// never permission to command those joints. Preparation is separately opt-in.
    #[allow(clippy::too_many_arguments)]
    pub fn selected(
        record: [u8; 32],
        calibration: UnitCalibration,
        limits: [JointEnvelope; JOINT_COUNT],
        valid_from: MonoTime,
        expires_at: MonoTime,
        allowed: JointSelection,
        prepare_from: Option<[RawProfile; JOINT_COUNT]>,
    ) -> Result<Self, Error> {
        if record == [0; 32] || expires_at <= valid_from {
            return Err(Error::Qualification);
        }
        for j in Joint::ALL {
            if allowed.contains(j) && !limits[j.index()].profile.bounded() {
                return Err(Error::Profile(j));
            }
        }
        if let Some(initial) = prepare_from {
            // This bounded setup operation is ID5-only. Other profile changes
            // cannot be represented by this qualification.
            if allowed.mask() != 16 {
                return Err(Error::Selection);
            }
            for j in Joint::ALL {
                if !allowed.contains(j) && initial[j.index()] != limits[j.index()].profile {
                    return Err(Error::Profile(j));
                }
            }
            if initial[4].goal_time_raw() != limits[4].profile.goal_time_raw() {
                return Err(Error::Profile(Joint::WristPitch));
            }
        }
        Ok(Self {
            record,
            calibration,
            limits,
            valid_from,
            expires_at,
            allowed,
            prepare_from,
        })
    }
    fn check_selection(&self, selection: JointSelection) -> Result<(), Error> {
        if selection.mask() & !self.allowed.mask() != 0 {
            return Err(Error::Selection);
        }
        for j in Joint::ALL {
            if selection.contains(j) && !self.limits[j.index()].profile.bounded() {
                return Err(Error::Profile(j));
            }
        }
        Ok(())
    }
}

/// Retain the actual placement observation's original camera grant and host
/// acquisition timestamp. The trusted producer must inspect placement/clearance;
/// this type cannot derive those facts from a frame number. No fresh token may
/// be attached to old bytes. Labels are not authenticated camera identity.
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    pub source_boot: BootId,
    pub frame_sequence: u64,
    pub acquired_at: MonoTime,
    pub valid_until: MonoTime,
    pub camera: CameraGrant,
    pub qualification_record: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MotionId {
    worker_boot: BootId,
    owner: TurnOwner,
    sequence: u64,
}
impl MotionId {
    pub fn worker_boot(self) -> BootId {
        self.worker_boot
    }
    pub fn owner(self) -> TurnOwner {
        self.owner
    }
    pub fn sequence(self) -> u64 {
        self.sequence
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    Read,
    Write,
    Discard,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Configuration,
    Qualification,
    Selection,
    ProfilePreparation,
    Placement,
    NotInspected,
    Busy,
    Stopped,
    Calibration(CalibrationError),
    Trajectory(TrajectoryError),
    Authority(lamp_turn_owner::Error),
    ClockRegression,
    TimeOverflow,
    Io {
        operation: Operation,
        kind: io::ErrorKind,
    },
    IoOverrun,
    Disconnected,
    InvalidIoCount,
    WriteZero,
    UnexpectedTraffic,
    Protocol(ReplyError),
    Deadline {
        pending_joint_mask: u8,
    },
    OperationLimit,
    Model(Joint),
    Readback(Joint),
    Profile(Joint),
    Torque(Joint),
    Position(Joint),
    Load(Joint),
    Velocity(Joint),
    Tracking(Joint),
    NotAtRest(Joint),
    ReadbackExpired,
    SegmentExpired,
    StaleMotion,
    CounterExhausted,
    ControlLost,
    Cancelled,
    Shutdown,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}

/// A terminal report, never evidence that the physical joint stopped or that
/// torque is safe to release. Local pending work is cleared before discard.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StopReport {
    pub reason: Error,
    pub profile_preparation: Option<ProfileProgress>,
    /// Includes a full request accepted just before an overrun/expiry was
    /// observed. Zero partial bytes alone must not imply no packet was sent.
    pub accepted_request_bytes: usize,
    pub position_request: bool,
    pub partial_request_bytes: usize,
    pub discard_error: Option<Error>,
    pub stopped_at: MonoTime,
}

/// Actual checked telemetry from one complete all-five read transaction. Times
/// are host monotonic receipt observations, not device acquisition timestamps.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EncoderReadback {
    pub counts: u16,
    pub received_at_us: u64,
    pub velocity_raw: i16,
    pub load_magnitude_raw: u16,
    pub moving_raw: u8,
    pub status_raw: u8,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Readback {
    /// Fixed bus order: IDs 1 through 5. Never a requested goal or a cached pose.
    pub joints: [EncoderReadback; JOINT_COUNT],
}
impl Readback {
    pub fn validate(self, at: MonoTime, maximum_age_us: u64) -> Result<(), Error> {
        for joint in Joint::ALL {
            let sample = self.joints[joint.index()];
            EncoderCounts::new(i32::from(sample.counts)).map_err(|_| Error::Position(joint))?;
            if age(at, MonoTime::from_micros(sample.received_at_us))? > maximum_age_us {
                return Err(Error::ReadbackExpired);
            }
            if sample.status_raw != 0
                || sample.moving_raw > 1
                || sample.load_magnitude_raw > 1023
                || sample.velocity_raw == i16::MIN
            {
                return Err(Error::Readback(joint));
            }
        }
        Ok(())
    }
    pub fn stationary(self) -> bool {
        self.joints
            .iter()
            .all(|s| s.moving_raw == 0 && s.velocity_raw == 0)
    }
    pub fn counts(self) -> [u16; JOINT_COUNT] {
        self.joints.map(|s| s.counts)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tick {
    ProfilePrepared {
        motion: MotionId,
        at: MonoTime,
        progress: ProfileProgress,
    },
    Idle,
    Progress,
    Inspected,
    /// Host accepted the complete selected-ID frame. No individual status ACK.
    PositionAccepted {
        selection: JointSelection,
        motion: MotionId,
        at: MonoTime,
        counts: [EncoderCounts; JOINT_COUNT],
    },
    /// All five checked host readbacks are stationary and within the explicitly
    /// configured endpoint tolerance. Not an optical clearance/pose assertion.
    Reached {
        motion: MotionId,
        at: MonoTime,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReadStage {
    Model,
    Limits,
    Homing,
    Profile,
    Telemetry,
}
impl ReadStage {
    fn window(self) -> ReadWindow {
        match self {
            Self::Model => ReadWindow::MODEL_NUMBER,
            Self::Limits => ReadWindow::POSITION_LIMITS,
            Self::Homing => ReadWindow::HOMING_AND_MODE,
            Self::Profile => ReadWindow::TORQUE_AND_PROFILE,
            Self::Telemetry => ReadWindow::TELEMETRY,
        }
    }
    fn next(self) -> Option<Self> {
        match self {
            Self::Model => Some(Self::Limits),
            Self::Limits => Some(Self::Homing),
            Self::Homing => Some(Self::Profile),
            Self::Profile => Some(Self::Telemetry),
            Self::Telemetry => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Sample {
    value: RawTelemetry,
    received_at: MonoTime,
}

struct Transaction {
    bytes: [u8; POSITION_FRAME_BYTES],
    length: usize,
    written: usize,
    stage: Option<ReadStage>,
    parser: StatusParser,
    replies: Option<ReplyTracker>,
    entered_at: MonoTime,
    operations: u16,
    counts: Option<[EncoderCounts; JOINT_COUNT]>,
    profile: Option<profile::Register>,
}
impl Transaction {
    fn read(stage: ReadStage, now: MonoTime) -> Self {
        let request =
            ReadOnlyRequest::sync_read(&Joint::ALL, stage.window()).expect("five distinct IDs");
        let mut bytes = [0; POSITION_FRAME_BYTES];
        bytes[..request.bytes().len()].copy_from_slice(request.bytes());
        Self {
            bytes,
            length: request.bytes().len(),
            written: 0,
            stage: Some(stage),
            parser: StatusParser::default(),
            replies: Some(request.track_replies()),
            entered_at: now,
            operations: 0,
            counts: None,
            profile: None,
        }
    }
    fn position(
        selection: JointSelection,
        counts: [EncoderCounts; JOINT_COUNT],
        now: MonoTime,
    ) -> Self {
        let frame = SelectedPositionFrame::encode(selection, counts);
        let mut bytes = [0; POSITION_FRAME_BYTES];
        bytes[..frame.bytes().len()].copy_from_slice(frame.bytes());
        Self {
            bytes,
            length: frame.bytes().len(),
            written: 0,
            stage: None,
            parser: StatusParser::default(),
            replies: None,
            entered_at: now,
            operations: 0,
            counts: Some(counts),
            profile: None,
        }
    }
    fn pending_mask(&self) -> u8 {
        self.replies.as_ref().map_or(0, |tracker| {
            tracker
                .pending_joints()
                .fold(0, |mask, joint| mask | joint.mask())
        })
    }
}

#[derive(Clone, Copy)]
struct Active {
    selection: JointSelection,
    id: MotionId,
    permit: OutputPermit,
    placement: Placement,
    segment: Segment,
    started_at: MonoTime,
    next_at: MonoTime,
    need_readback: bool,
    final_written: bool,
    last_position: [NormalizedPosition; JOINT_COUNT],
    last_velocity: [f64; JOINT_COUNT],
    last_write_at: MonoTime,
    endpoint_counts: [EncoderCounts; JOINT_COUNT],
}

/// One owner of one port, one transaction and one finite segment. There is no
/// command queue, retry, automatic torque-off, recovery, or background motion.
pub struct MotorControl<P: BusPort, C: Clock> {
    port: P,
    clock: C,
    worker_boot: BootId,
    calibration: UnitCalibration,
    timing: Timing,
    guard: BoundaryGuard,
    last_now: MonoTime,
    qualification: Option<QualifiedEnvelope>,
    quiet: Option<(MonoTime, MonoTime)>,
    inspect_next: Option<ReadStage>,
    inspected_at: Option<MonoTime>,
    transaction: Option<Transaction>,
    limits: [Option<PositionLimitRegisters>; JOINT_COUNT],
    homing: [Option<HomingModeRegisters>; JOINT_COUNT],
    profiles: [Option<[u8; 8]>; JOINT_COUNT],
    samples: [Option<Sample>; JOINT_COUNT],
    active: Option<Active>,
    preparing: Option<Preparing>,
    profile_progress: Option<ProfileProgress>,
    sequence: u64,
    terminal: Option<StopReport>,
}

impl<P: BusPort, C: Clock> MotorControl<P, C> {
    pub fn new(
        port: P,
        clock: C,
        controller_boot: BootId,
        worker_boot: BootId,
        expected_unit: &UnitId,
        calibration: UnitCalibration,
        timing: Timing,
    ) -> Result<Self, Error> {
        calibration
            .bind(expected_unit)
            .map_err(Error::Calibration)?;
        let now = clock.now();
        Ok(Self {
            port,
            clock,
            worker_boot,
            calibration,
            timing,
            guard: BoundaryGuard::new(controller_boot, now),
            last_now: now,
            qualification: None,
            quiet: None,
            inspect_next: None,
            inspected_at: None,
            transaction: None,
            limits: [None; JOINT_COUNT],
            homing: [None; JOINT_COUNT],
            profiles: [None; JOINT_COUNT],
            samples: [None; JOINT_COUNT],
            active: None,
            preparing: None,
            profile_progress: None,
            sequence: 0,
            terminal: None,
        })
    }

    /// Only the owned Linux adapter uses this for explicit resource cleanup.
    /// No public access to the raw transport bypasses the command boundary.
    #[cfg(unix)]
    pub(crate) fn port_mut(&mut self) -> &mut P {
        &mut self.port
    }

    pub fn stop_report(&self) -> Option<StopReport> {
        self.terminal
    }

    pub fn install(&mut self, snapshot: Snapshot) -> Result<(), Error> {
        let now = self.now()?;
        if let Err(error) = self.guard.install(now, snapshot) {
            if self.guard.is_faulted() {
                self.stop(Error::Authority(error), now);
            }
            return Err(Error::Authority(error));
        }
        if let Err(error) = self.check_active(now) {
            self.stop(error, now);
            return Err(error);
        }
        Ok(())
    }

    pub fn qualify(&mut self, evidence: QualifiedEnvelope) -> Result<(), Error> {
        let now = self.now()?;
        if self.active.is_some() || self.preparing.is_some() || self.profile_progress.is_some() {
            return Err(Error::Busy);
        }
        if evidence.calibration != self.calibration
            || evidence.valid_from > now
            || evidence.expires_at <= now
        {
            return Err(Error::Qualification);
        }
        self.qualification = Some(evidence);
        Ok(())
    }

    /// Explicit read-only initialization/refresh; no automatic probe or retry.
    /// Twenty milliseconds observed quiet is not proof of absent delayed data.
    /// No bus I/O. Exports only the current complete checked transaction, never
    /// substitutes goals for readback. A fault, partial read or expiry rejects it.
    pub fn readback(&mut self) -> Result<Readback, Error> {
        let now = self.now()?;
        if self.inspected_at.is_none() || self.inspect_next.is_some() || self.transaction.is_some()
        {
            return Err(Error::NotInspected);
        }
        let mut joints = [EncoderReadback {
            counts: 0,
            received_at_us: 0,
            velocity_raw: 0,
            load_magnitude_raw: 0,
            moving_raw: 0,
            status_raw: 0,
        }; JOINT_COUNT];
        for joint in Joint::ALL {
            let sample = self.samples[joint.index()].ok_or(Error::NotInspected)?;
            self.position(joint, sample.value)?;
            if self.qualification.is_some() {
                self.check_sample(joint, sample, now, None)?;
            }
            joints[joint.index()] = EncoderReadback {
                counts: sample
                    .value
                    .position()
                    .single_turn_counts()
                    .map_err(|_| Error::Position(joint))?
                    .get(),
                received_at_us: sample.received_at.as_micros(),
                velocity_raw: sample.value.velocity().signed_raw(),
                load_magnitude_raw: sample.value.load().magnitude_raw(),
                moving_raw: sample.value.moving_raw(),
                status_raw: sample.value.status_raw(),
            };
        }
        let result = Readback { joints };
        result.validate(now, self.timing.readback_age_us)?;
        Ok(result)
    }
    pub fn inspect(&mut self) -> Result<(), Error> {
        let now = self.now()?;
        if self.transaction.is_some()
            || self.active.is_some()
            || self.preparing.is_some()
            || self.quiet.is_some()
            || self.inspect_next.is_some()
        {
            return Err(Error::Busy);
        }
        self.inspected_at = None;
        self.limits.fill(None);
        self.homing.fill(None);
        self.profiles.fill(None);
        self.samples.fill(None);
        self.quiet = Some((now, add(now, QUIET_US)?));
        Ok(())
    }

    pub fn start_segment(
        &mut self,
        permit: OutputPermit,
        placement: Placement,
        target: [NormalizedPosition; JOINT_COUNT],
        duration_us: u64,
    ) -> Result<MotionId, Error> {
        self.start_selected_segment(permit, placement, target, duration_us, JointSelection::ALL)
    }

    /// Start a finite segment that writes only the selected joint IDs. All five
    /// remain inspected and qualified. Unselected goals retain exact read counts,
    /// not a potentially lossy normalize/denormalize round trip.
    pub fn start_selected_segment(
        &mut self,
        permit: OutputPermit,
        placement: Placement,
        target: [NormalizedPosition; JOINT_COUNT],
        duration_us: u64,
        selection: JointSelection,
    ) -> Result<MotionId, Error> {
        let result = self.start_segment_inner(permit, placement, target, duration_us, selection);
        // Bad requests may be corrected. A discovered device/readback fault
        // cannot be healed by changing the request or qualification record.
        if let Err(
            error @ (Error::Torque(_)
            | Error::Profile(_)
            | Error::Readback(_)
            | Error::Position(_)
            | Error::Load(_)
            | Error::Velocity(_)
            | Error::Tracking(_)
            | Error::Calibration(_)),
        ) = result
        {
            self.stop(error, self.clock.now());
        }
        result
    }

    fn start_segment_inner(
        &mut self,
        permit: OutputPermit,
        placement: Placement,
        mut target: [NormalizedPosition; JOINT_COUNT],
        duration_us: u64,
        selection: JointSelection,
    ) -> Result<MotionId, Error> {
        let now = self.now()?;
        if self.transaction.is_some()
            || self.active.is_some()
            || self.preparing.is_some()
            || self.quiet.is_some()
            || self.inspect_next.is_some()
        {
            return Err(Error::Busy);
        }
        self.guard
            .check(now, permit, OutputKind::Motion)
            .map_err(Error::Authority)?;
        self.check_placement(now, permit, placement)?;
        let qualification = self.qualification.as_ref().ok_or(Error::Qualification)?;
        qualification.check_selection(selection)?;
        let inspected = self.inspected_at.ok_or(Error::NotInspected)?;
        if age(now, inspected)? > self.timing.readback_age_us {
            return Err(Error::ReadbackExpired);
        }
        let bound = self
            .calibration
            .bind(self.calibration.unit_id())
            .map_err(Error::Calibration)?;
        let mut start = target;
        let mut quantized_target = target;
        let mut endpoint_counts =
            Joint::ALL.map(|joint| bound.joint(joint).denormalize(target[joint.index()]));
        for joint in Joint::ALL {
            let i = joint.index();
            let limits = qualification.limits[i];
            let profile = self.profiles[i].ok_or(Error::NotInspected)?;
            // A position command must not silently re-engage a disabled servo.
            // Never repair torque, speed or acceleration as part of admission.
            if profile[0] != 1 {
                return Err(Error::Torque(joint));
            }
            if !limits.profile.matches(&profile[1..]) {
                return Err(Error::Profile(joint));
            }
            let sample = self.samples[i].ok_or(Error::NotInspected)?;
            self.check_sample(joint, sample, now, None)?;
            if sample.value.moving_raw() != 0 || sample.value.velocity().signed_raw() != 0 {
                return Err(Error::NotAtRest(joint));
            }
            start[i] = self.position(joint, sample.value)?;
            if qualification.allowed != JointSelection::ALL
                && u16::from_le_bytes([profile[2], profile[3]])
                    != sample
                        .value
                        .position()
                        .single_turn_counts()
                        .map_err(|_| Error::Position(joint))?
                        .get()
            {
                return Err(Error::NotAtRest(joint));
            }
            if !selection.contains(joint) {
                target[i] = start[i];
                endpoint_counts[i] = sample
                    .value
                    .position()
                    .single_turn_counts()
                    .map_err(|_| Error::Position(joint))?;
            }
            if !bound
                .joint(joint)
                .register_binding()
                .contains(endpoint_counts[i])
            {
                return Err(Error::Position(joint));
            }
            quantized_target[i] = bound
                .joint(joint)
                .normalize(endpoint_counts[i])
                .map_err(Error::Calibration)?;
            // Quantization must not widen the envelope or conceal a bad request.
            if !limits.contains(target[i]) || !limits.contains(quantized_target[i]) {
                return Err(Error::Trajectory(TrajectoryError::Range(joint)));
            }
        }
        let segment = Segment::new(start, quantized_target, duration_us, &qualification.limits)
            .map_err(Error::Trajectory)?;
        let finish = add(add(now, duration_us)?, self.timing.transaction_us)?;
        if finish >= permit.expires_at()
            || finish >= placement.valid_until
            || finish >= qualification.expires_at
        {
            return Err(Error::SegmentExpired);
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or(Error::CounterExhausted)?;
        let id = MotionId {
            worker_boot: self.worker_boot,
            owner: permit.owner(),
            sequence: self.sequence,
        };
        self.active = Some(Active {
            selection,
            id,
            permit,
            placement,
            segment,
            started_at: now,
            next_at: add(now, self.timing.interval_us)?,
            need_readback: false,
            final_written: false,
            last_position: start,
            last_velocity: [0.0; JOINT_COUNT],
            last_write_at: now,
            endpoint_counts,
        });
        Ok(id)
    }

    pub fn cancel(&mut self, motion: MotionId) -> Result<StopReport, Error> {
        let now = self.now()?;
        if !self.active.is_some_and(|active| active.id == motion)
            && !self.preparing.is_some_and(|p| p.id == motion)
        {
            return Err(Error::StaleMotion);
        }
        Ok(self.stop(Error::Cancelled, now))
    }

    pub fn shutdown(&mut self) -> StopReport {
        if let Some(report) = self.terminal {
            return report;
        }
        self.stop(Error::Shutdown, self.clock.now())
    }

    /// Decode failure, trusted control channel loss, or supervisor cancellation
    /// invalidates cached authority immediately. No reconnect/rebind here.
    pub fn control_lost(&mut self) -> StopReport {
        self.stop(Error::ControlLost, self.clock.now())
    }

    /// At most one 128-byte read and one <=23-byte write; no waits or retries.
    /// The worker must call this at least every 20 ms and prioritize cancellation.
    pub fn tick(&mut self) -> Result<Tick, Error> {
        let result = self.tick_inner();
        if let Err(error) = result
            && self.terminal.is_none()
        {
            self.stop(error, self.clock.now());
        }
        result
    }

    fn tick_inner(&mut self) -> Result<Tick, Error> {
        let now = self.now()?;
        self.check_active(now)?;
        if let Some((entered, until)) = self.quiet {
            if age(now, entered)? > 200_000 {
                return Err(Error::Deadline {
                    pending_joint_mask: 0,
                });
            }
            if self.read(now)?.is_some() {
                return Err(Error::UnexpectedTraffic);
            }
            let now = self.now()?;
            if now >= until {
                self.quiet = None;
                self.inspect_next = Some(ReadStage::Model);
            }
            return Ok(Tick::Progress);
        }
        if self.transaction.is_some() {
            return self.step_transaction(now);
        }
        if let Some(stage) = self.inspect_next {
            self.transaction = Some(Transaction::read(stage, now));
            return Ok(Tick::Progress);
        }
        if self.preparing.is_some() {
            return self.queue_profile_write(now);
        }
        if let Some(active) = self.active {
            if active.need_readback {
                self.transaction = Some(Transaction::read(ReadStage::Telemetry, now));
                return Ok(Tick::Progress);
            }
            if now >= active.next_at {
                let elapsed = age(now, active.started_at)?;
                let sample = active.segment.sample(elapsed);
                let bound = self
                    .calibration
                    .bind(self.calibration.unit_id())
                    .map_err(Error::Calibration)?;
                let counts = if elapsed >= active.segment.duration_us() {
                    active.endpoint_counts
                } else {
                    Joint::ALL.map(|joint| {
                        if active.selection.contains(joint) {
                            bound.joint(joint).denormalize(sample[joint.index()])
                        } else {
                            active.endpoint_counts[joint.index()]
                        }
                    })
                };
                self.check_goal(now, counts)?;
                self.transaction = Some(Transaction::position(active.selection, counts, now));
                return Ok(Tick::Progress);
            }
        }
        Ok(Tick::Idle)
    }

    fn step_transaction(&mut self, now: MonoTime) -> Result<Tick, Error> {
        self.check_transaction(now)?;
        if self
            .transaction
            .as_ref()
            .expect("active transaction")
            .written
            < self
                .transaction
                .as_ref()
                .expect("active transaction")
                .length
        {
            if self.read(now)?.is_some() {
                return Err(Error::UnexpectedTraffic);
            }
            let now = self.now()?;
            self.check_transaction(now)?;
            self.check_active(now)?;
            if let Some(counts) = self.transaction.as_ref().and_then(|tx| tx.counts) {
                self.check_goal(now, counts)?;
            }
            let tx = self.transaction.as_mut().expect("active transaction");
            tx.operations = tx.operations.checked_add(1).ok_or(Error::OperationLimit)?;
            let remaining = tx.length - tx.written;
            let result = self.port.write(&tx.bytes[tx.written..tx.length]);
            // Preserve accepted-byte accounting even if time/authority fails.
            match result {
                Ok(0) => return Err(Error::WriteZero),
                Ok(count) if count <= remaining => {
                    self.transaction
                        .as_mut()
                        .expect("active transaction")
                        .written += count
                }
                Ok(_) => return Err(Error::InvalidIoCount),
                Err(error) if transient(&error) => {}
                Err(error) => return Err(io_error(Operation::Write, error)),
            }
            self.profile_accepted_bytes();
            let after = self.after_io(now)?;
            self.check_transaction(after)?;
            self.check_active(after)?;
            let tx = self.transaction.as_ref().expect("active transaction");
            if tx.written == tx.length
                && let Some(counts) = tx.counts
            {
                // Use the actual host-acceptance time for the next discrete step.
                let velocity = self.check_goal(after, counts)?;
                let positions = self.positions(counts)?;
                let active = self.active.as_mut().expect("position has an owner");
                active.last_velocity = velocity;
                active.last_position = positions;
                active.last_write_at = after;
                active.next_at = add(after, self.timing.interval_us)?;
                active.need_readback = true;
                active.final_written =
                    age(tx.entered_at, active.started_at)? >= active.segment.duration_us();
                let id = active.id;
                let selection = active.selection;
                self.transaction = None;
                return Ok(Tick::PositionAccepted {
                    selection,
                    motion: id,
                    at: after,
                    counts,
                });
            }
            return Ok(Tick::Progress);
        }
        if let Some((bytes, count, after)) = self.read(now)? {
            self.check_transaction(after)?;
            self.check_active(after)?;
            let stage = self.transaction.as_ref().and_then(|tx| tx.stage);
            let mut replies = [None, None, None, None, None];
            let mut error = None;
            let tx = self.transaction.as_mut().expect("active transaction");
            let tracker = tx.replies.as_mut().expect("read tracker");
            let report = tx
                .parser
                .feed(&bytes[..count], |event| match tracker.accept(event) {
                    Ok(reply) => {
                        let i = reply.joint().index();
                        replies[i] = Some(reply);
                    }
                    Err(fault) => {
                        error.get_or_insert(Error::Protocol(fault));
                    }
                });
            if let Some(error) = error {
                return Err(error);
            }
            if report.discarded_bytes != 0 {
                return Err(Error::UnexpectedTraffic);
            }
            if stage.is_none() {
                let tx = self.transaction.as_ref().expect("profile transaction");
                if tx.profile.is_none() {
                    return Err(Error::ProfilePreparation);
                }
                if tx.replies.as_ref().expect("ACK tracker").is_complete() {
                    if tx.parser.buffered_bytes() != 0 {
                        return Err(Error::UnexpectedTraffic);
                    }
                    return self.profile_ack(after);
                }
                return Ok(Tick::Progress);
            }
            let stage = stage.expect("checked read stage");
            for reply in replies.into_iter().flatten() {
                let joint = reply.joint();
                let i = joint.index();
                match stage {
                    ReadStage::Model => {
                        if reply
                            .model_number_raw()
                            .map_err(|_| Error::Readback(joint))?
                            != STS3215_MODEL_NUMBER
                        {
                            return Err(Error::Model(joint));
                        }
                    }
                    ReadStage::Limits => {
                        self.limits[i] = Some(
                            reply
                                .position_limits()
                                .map_err(|_| Error::Readback(joint))?,
                        )
                    }
                    ReadStage::Homing => {
                        let homing = reply
                            .homing_and_mode()
                            .map_err(|_| Error::Readback(joint))?;
                        self.calibration
                            .bind(self.calibration.unit_id())
                            .map_err(Error::Calibration)?
                            .joint(joint)
                            .compare_registers(self.limits[i].ok_or(Error::NotInspected)?, homing)
                            .map_err(Error::Calibration)?;
                        self.homing[i] = Some(homing);
                    }
                    ReadStage::Profile => {
                        let value = reply
                            .torque_and_profile()
                            .map_err(|_| Error::Readback(joint))?;
                        self.profiles[i] = Some(*value.bytes());
                    }
                    ReadStage::Telemetry => {
                        let value = reply.telemetry().map_err(|_| Error::Readback(joint))?;
                        // These flags do not need a qualified numeric threshold.
                        if value.status_raw() != 0 || value.moving_raw() > 1 {
                            return Err(Error::Readback(joint));
                        }
                        if value.load().uninterpreted_bits() != 0 {
                            return Err(Error::Load(joint));
                        }
                        let sample = Sample {
                            value,
                            received_at: after,
                        };
                        self.position(joint, value)?;
                        if self.qualification.is_some() {
                            self.check_sample(
                                joint,
                                sample,
                                after,
                                self.active.map(|a| a.last_position[i]),
                            )?;
                        }
                        self.samples[i] = Some(sample);
                    }
                }
            }
        }
        let tx = self.transaction.as_ref().expect("active transaction");
        if !tx.replies.as_ref().expect("read tracker").is_complete() {
            return Ok(Tick::Progress);
        }
        if tx.parser.buffered_bytes() != 0 {
            return Err(Error::UnexpectedTraffic);
        }
        let stage = tx.stage.expect("read stage");
        self.transaction = None;
        let now = self.now()?;
        if self.inspect_next.is_some() {
            self.inspect_next = stage.next();
            if self.inspect_next.is_none() {
                self.inspected_at = Some(now);
                if self.preparing.is_some() {
                    return self.profile_readback(now);
                }
                return Ok(Tick::Inspected);
            }
        } else if let Some(active) = self.active {
            let settled = active.final_written && self.is_settled(now, active.last_position)?;
            if settled {
                self.active = None;
                return Ok(Tick::Reached {
                    motion: active.id,
                    at: now,
                });
            }
            self.active.as_mut().expect("active segment").need_readback = active.final_written;
        }
        Ok(Tick::Progress)
    }

    fn read(
        &mut self,
        before: MonoTime,
    ) -> Result<Option<([u8; READ_BYTES], usize, MonoTime)>, Error> {
        if let Some(tx) = &mut self.transaction {
            tx.operations = tx.operations.checked_add(1).ok_or(Error::OperationLimit)?;
        }
        let mut bytes = [0; READ_BYTES];
        let result = self.port.read(&mut bytes);
        let after = self.after_io(before)?;
        match result {
            Ok(0) => Err(Error::Disconnected),
            Ok(count) if count <= READ_BYTES => Ok(Some((bytes, count, after))),
            Ok(_) => Err(Error::InvalidIoCount),
            Err(error) if transient(&error) => Ok(None),
            Err(error) => Err(io_error(Operation::Read, error)),
        }
    }

    fn check_transaction(&self, now: MonoTime) -> Result<(), Error> {
        let tx = self.transaction.as_ref().expect("active transaction");
        if age(now, tx.entered_at)? >= self.timing.transaction_us {
            return Err(Error::Deadline {
                pending_joint_mask: tx.pending_mask(),
            });
        }
        if tx.operations >= MAX_OPERATIONS {
            return Err(Error::OperationLimit);
        }
        Ok(())
    }

    fn check_active(&mut self, now: MonoTime) -> Result<(), Error> {
        self.check_preparing(now)?;
        if let Some(active) = self.active {
            self.qualification
                .as_ref()
                .ok_or(Error::Qualification)?
                .check_selection(active.selection)?;
            self.guard
                .check(now, active.permit, OutputKind::Motion)
                .map_err(Error::Authority)?;
            self.check_placement(now, active.permit, active.placement)?;
            if age(now, active.started_at)?
                >= active.segment.duration_us() + self.timing.transaction_us
            {
                return Err(Error::SegmentExpired);
            }
            for joint in Joint::ALL {
                self.check_sample(
                    joint,
                    self.samples[joint.index()].ok_or(Error::NotInspected)?,
                    now,
                    None,
                )?;
            }
        }
        Ok(())
    }

    fn check_placement(
        &mut self,
        now: MonoTime,
        permit: OutputPermit,
        placement: Placement,
    ) -> Result<(), Error> {
        let evidence = self.qualification.as_ref().ok_or(Error::Qualification)?;
        if now < evidence.valid_from || now >= evidence.expires_at {
            return Err(Error::Qualification);
        }
        if placement.frame_sequence == 0
            || placement.qualification_record != evidence.record
            || placement.acquired_at > now
            || placement.valid_until <= now
            || placement.valid_until <= placement.acquired_at
            || age(now, placement.acquired_at)? > self.timing.placement_age_us
            || age(placement.valid_until, placement.acquired_at)? > self.timing.placement_age_us
            || permit.camera() != Some(placement.camera)
            || self
                .guard
                .current_camera_grant(now)
                .map_err(Error::Authority)?
                != placement.camera
        {
            return Err(Error::Placement);
        }
        Ok(())
    }

    fn position(&self, joint: Joint, value: RawTelemetry) -> Result<NormalizedPosition, Error> {
        let counts = value
            .position()
            .single_turn_counts()
            .map_err(|_| Error::Position(joint))?;
        let calibration = self
            .calibration
            .bind(self.calibration.unit_id())
            .map_err(Error::Calibration)?
            .joint(joint);
        let normalized = calibration.normalize(counts).map_err(Error::Calibration)?;
        if !calibration.register_binding().contains(counts) {
            return Err(Error::Position(joint));
        }
        Ok(normalized)
    }

    fn positions(
        &self,
        counts: [EncoderCounts; JOINT_COUNT],
    ) -> Result<[NormalizedPosition; JOINT_COUNT], Error> {
        let mut result = [NormalizedPosition::new(0.0).expect("finite zero"); JOINT_COUNT];
        let bound = self
            .calibration
            .bind(self.calibration.unit_id())
            .map_err(Error::Calibration)?;
        for joint in Joint::ALL {
            if !bound
                .joint(joint)
                .register_binding()
                .contains(counts[joint.index()])
            {
                return Err(Error::Position(joint));
            }
            result[joint.index()] = bound
                .joint(joint)
                .normalize(counts[joint.index()])
                .map_err(Error::Calibration)?;
        }
        Ok(result)
    }

    fn check_sample(
        &self,
        joint: Joint,
        sample: Sample,
        now: MonoTime,
        target: Option<NormalizedPosition>,
    ) -> Result<(), Error> {
        if age(now, sample.received_at)? > self.timing.readback_age_us {
            return Err(Error::ReadbackExpired);
        }
        let value = sample.value;
        let limit = self
            .qualification
            .as_ref()
            .ok_or(Error::Qualification)?
            .limits[joint.index()];
        if value.status_raw() != 0 || value.moving_raw() > 1 {
            return Err(Error::Readback(joint));
        }
        let position = self.position(joint, value)?;
        if !limit.contains(position) {
            return Err(Error::Position(joint));
        }
        if value.load().uninterpreted_bits() != 0
            || value.load().magnitude_raw() > limit.maximum_load_raw
        {
            return Err(Error::Load(joint));
        }
        if value.velocity().signed_raw().unsigned_abs() > limit.maximum_velocity_raw {
            return Err(Error::Velocity(joint));
        }
        if target.is_some_and(|target| (position.get() - target.get()).abs() > limit.tracking_error)
        {
            return Err(Error::Tracking(joint));
        }
        Ok(())
    }

    fn check_goal(
        &self,
        now: MonoTime,
        counts: [EncoderCounts; JOINT_COUNT],
    ) -> Result<[f64; JOINT_COUNT], Error> {
        let active = self.active.ok_or(Error::StaleMotion)?;
        self.qualification
            .as_ref()
            .ok_or(Error::Qualification)?
            .check_selection(active.selection)?;
        let positions = self.positions(counts)?;
        let seconds = age(now, active.last_write_at)? as f64 / 1_000_000.0;
        if seconds <= 0.0 {
            return Err(Error::ClockRegression);
        }
        let mut velocity = [0.0; JOINT_COUNT];
        for joint in Joint::ALL {
            let i = joint.index();
            let limits = self
                .qualification
                .as_ref()
                .ok_or(Error::Qualification)?
                .limits[i];
            if !limits.contains(positions[i]) {
                return Err(Error::Trajectory(TrajectoryError::Range(joint)));
            }
            velocity[i] = (positions[i].get() - active.last_position[i].get()) / seconds;
            if velocity[i].abs() > limits.velocity {
                return Err(Error::Trajectory(TrajectoryError::Velocity(joint)));
            }
            if ((velocity[i] - active.last_velocity[i]) / seconds).abs() > limits.acceleration {
                return Err(Error::Trajectory(TrajectoryError::Acceleration(joint)));
            }
        }
        Ok(velocity)
    }

    fn is_settled(
        &self,
        now: MonoTime,
        target: [NormalizedPosition; JOINT_COUNT],
    ) -> Result<bool, Error> {
        let mut settled = true;
        for joint in Joint::ALL {
            let i = joint.index();
            let sample = self.samples[i].ok_or(Error::NotInspected)?;
            self.check_sample(joint, sample, now, Some(target[i]))?;
            let error = (self.position(joint, sample.value)?.get() - target[i].get()).abs();
            settled &= sample.value.moving_raw() == 0
                && sample.value.velocity().signed_raw() == 0
                && error
                    <= self
                        .qualification
                        .as_ref()
                        .ok_or(Error::Qualification)?
                        .limits[i]
                        .settling_error;
        }
        Ok(settled)
    }

    fn now(&mut self) -> Result<MonoTime, Error> {
        if self.terminal.is_some() {
            return Err(Error::Stopped);
        }
        let now = self.clock.now();
        if now < self.last_now {
            self.stop(Error::ClockRegression, now);
            return Err(Error::ClockRegression);
        }
        self.last_now = now;
        Ok(now)
    }
    fn after_io(&mut self, before: MonoTime) -> Result<MonoTime, Error> {
        let after = self.now()?;
        if age(after, before)? > self.timing.io_us {
            return Err(Error::IoOverrun);
        }
        Ok(after)
    }
    fn stop(&mut self, reason: Error, now: MonoTime) -> StopReport {
        if let Some(report) = self.terminal {
            return report;
        }
        let partial = self
            .transaction
            .as_ref()
            .filter(|tx| tx.written < tx.length)
            .map_or(0, |tx| tx.written);
        let report = StopReport {
            reason,
            profile_preparation: self.profile_progress,
            accepted_request_bytes: self.transaction.as_ref().map_or(0, |tx| tx.written),
            position_request: self
                .transaction
                .as_ref()
                .is_some_and(|tx| tx.counts.is_some()),
            partial_request_bytes: partial,
            discard_error: None,
            stopped_at: now,
        };
        // Latch first, before any cleanup that can itself fail or block.
        self.terminal = Some(report);
        self.active = None;
        self.preparing = None;
        self.transaction = None;
        self.quiet = None;
        self.inspect_next = None;
        self.inspected_at = None;
        self.samples.fill(None);
        self.qualification = None;
        self.guard.invalidate();
        let before = self.clock.now();
        let result = self
            .port
            .discard_output()
            .map_err(|error| io_error(Operation::Discard, error));
        let after = self.clock.now();
        let discard_error = result.err().or_else(|| match age(after, before) {
            Ok(elapsed) if elapsed > self.timing.io_us => Some(Error::IoOverrun),
            Err(error) => Some(error),
            _ => None,
        });
        let report = StopReport {
            discard_error,
            ..report
        };
        self.terminal = Some(report);
        report
    }
}

impl<P: BusPort, C: Clock> Drop for MotorControl<P, C> {
    fn drop(&mut self) {
        // Best effort only. Call shutdown explicitly to retain cleanup evidence.
        self.shutdown();
    }
}

fn add(time: MonoTime, micros: u64) -> Result<MonoTime, Error> {
    time.checked_add(micros).ok_or(Error::TimeOverflow)
}
fn age(now: MonoTime, earlier: MonoTime) -> Result<u64, Error> {
    now.as_micros()
        .checked_sub(earlier.as_micros())
        .ok_or(Error::ClockRegression)
}
fn transient(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
    )
}
fn io_error(operation: Operation, error: io::Error) -> Error {
    Error::Io {
        operation,
        kind: error.kind(),
    }
}
