//! Bounded capture for a supervised, dedicated microphone process. The device
//! underneath is a [`CaptureDevice`]: ALSA on the board, and anything scripted
//! in tests. Everything below is the same for both.
//!
//! Construction opens nothing. Install fresh trusted microphone authority, then
//! explicitly `start`. An open handle is not listening readiness or permission
//! to send audio to a provider. The supervisor must drain priority privacy state
//! first and call `maintain` at least every 10 ms, even with no consumer demand.
//! No provider calls, mixer changes, implicit device fallback, or ALSA recovery
//! happen here. One supervisor must own the device; an alias is not an exclusive
//! ownership mechanism. Use the explicitly provisioned alias (legacy primary capture was
//! `plug:device_micro2`; the installed board routing must be verified separately).
//!
//! Each read performs at most one nonblocking ALSA read into a fixed 160-sample
//! assembler. Requested buffers contain 2..=4 ten-millisecond blocks; accepted
//! periods are 1..=160 frames and buffers 320..=640 frames, capped by the request.
//! ALSA opening, configuration and kernel calls have no userspace hard deadline;
//! process supervision must enforce a stall timeout. Host read times and ALSA's
//! optional status clock are recorded separately. Neither is a measured acoustic
//! acquisition timestamp. A 10 ms read budget is checked after the call returns.
//!
//! Revocation, expiry and capture faults close the handle and erase local partial
//! audio. A caller must purge its own queued frames and reset AEC on discontinuity,
//! call `reset`, then explicitly `start` under fresh permitted authority. Returned
//! frames cannot be retracted by this library. Every reopen has a new epoch. A
//! microphone privacy-generation change closes capture even when a denied-to-
//! allowed interval was coalesced into one snapshot. Normal conversation changes
//! do not discard opening words. Control loss latches closed and requires a newly
//! supervised instance, not merely `reset`.
use crate::{
    CAPTURE_RATE, CAPTURE_SAMPLES, CaptureBlock,
    device::{CaptureDevice, ConfigureError, DeviceFault, DeviceState, errno},
};
use lamp_ipc::monotonic_us;
use lamp_turn_owner::{BootId, BoundaryGuard, MonoTime, Permission, Snapshot};
use serde::Serialize;
use std::fmt;

const MAX_READ_US: u64 = 10_000;
const MIN_BUFFER_FRAMES: i64 = (CAPTURE_SAMPLES * 2) as i64;
const MAX_BUFFER_FRAMES: i64 = (CAPTURE_SAMPLES * 4) as i64;

pub type Result<T> = std::result::Result<T, CaptureError>;

#[derive(Clone, Copy, Debug)]
pub struct CaptureConfig {
    buffer_blocks: u8,
}

impl CaptureConfig {
    pub fn new(buffer_blocks: u8) -> Result<Self> {
        if !(2..=4).contains(&buffer_blocks) {
            return Err(CaptureError::new(ErrorKind::InvalidConfig(
                "capture buffer must contain 2..=4 ten-millisecond blocks",
            )));
        }
        Ok(Self { buffer_blocks })
    }

    pub fn buffer_blocks(self) -> u8 {
        self.buffer_blocks
    }

    /// The buffer asked of the device, in frames.
    pub fn requested_frames(self) -> i64 {
        i64::from(self.buffer_blocks) * CAPTURE_SAMPLES as i64
    }
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self { buffer_blocks: 4 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct CaptureFormat {
    pub rate: u32,
    pub channels: u32,
    pub period_frames: i64,
    pub buffer_frames: i64,
    /// True only after ALSA accepts and reports enabled monotonic timestamps.
    pub monotonic_status_timestamps: bool,
}

impl CaptureFormat {
    fn buffer_us(self) -> u64 {
        self.buffer_frames as u64 * 1_000_000 / u64::from(self.rate)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Discontinuity {
    PrivacyRevoked,
    AuthorityExpired,
    ControlFault,
    ClockRegression,
    XRun,
    Suspended,
    Disconnected,
    DeviceError,
    PartialTimeout,
    ReadBudgetExceeded,
    InvalidRead,
    CounterExhausted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    InvalidConfig(&'static str),
    UnsupportedFormat(CaptureFormat),
    Authority(lamp_turn_owner::Error),
    NotPermitted,
    Closed,
    AlreadyOpen,
    NotStarted,
    ResetRequired(Discontinuity),
    Discontinuity(Discontinuity),
}

#[derive(Debug)]
pub struct CaptureError {
    pub kind: ErrorKind,
    pub device_error: Option<DeviceFault>,
    /// A failed device discard is reported even though the handle is then closed.
    pub close_error: Option<DeviceFault>,
}

impl CaptureError {
    fn new(kind: ErrorKind) -> Self {
        Self {
            kind,
            device_error: None,
            close_error: None,
        }
    }

    fn device(error: DeviceFault) -> Self {
        Self {
            kind: ErrorKind::Discontinuity(classify_device_error(error)),
            device_error: Some(error),
            close_error: None,
        }
    }
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "capture: {:?}", self.kind)?;
        if let Some(error) = &self.device_error {
            write!(f, ": {error}")?;
        }
        if let Some(error) = &self.close_error {
            write!(f, "; discard also failed: {error}")?;
        }
        Ok(())
    }
}

impl std::error::Error for CaptureError {}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct PrepareReport {
    pub epoch: u64,
    pub format: CaptureFormat,
    pub open_started_at_us: u64,
    pub prepared_at_us: u64,
    pub microphone_generation: u64,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct StartReport {
    pub epoch: u64,
    pub format: CaptureFormat,
    pub open_started_at_us: u64,
    pub prepared_at_us: u64,
    /// Host time immediately before snd_pcm_start, not ADC acquisition.
    pub capture_started_at_us: u64,
    pub microphone_generation: u64,
    /// Completion of start, not proof of retained audio or listening readiness.
    pub start_completed_at_us: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StateUpdate {
    /// Purge downstream frames/reset AEC on privacy closure, even if this is zero.
    pub partial_samples_discarded: usize,
    pub closed: bool,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct ReadTiming {
    /// First successful host read that contributed to this frame.
    pub first_read_started_at_us: u64,
    pub last_read_started_at_us: u64,
    pub read_completed_at_us: u64,
    pub successful_reads: u16,
    /// Driver-reported status clock, not sample acquisition time.
    pub alsa_status_monotonic_us: Option<u64>,
    /// Host time when the post-read status observation completed.
    pub status_observed_at_us: u64,
    pub available_frames: i64,
    pub delayed_frames: i64,
}

#[derive(Clone, Debug)]
pub struct CapturedFrame {
    pub samples: CaptureBlock,
    pub epoch: u64,
    pub sequence: u64,
    pub authority_generation: u64,
    pub microphone_generation: u64,
    pub timing: ReadTiming,
}

/// The microphone boundary over the board's ALSA device.
#[cfg(target_os = "linux")]
pub type Microphone = MicrophoneOn<crate::alsa_device::AlsaCapture>;

pub struct MicrophoneOn<D: CaptureDevice> {
    device: String,
    config: CaptureConfig,
    guard: BoundaryGuard,
    last_now: MonoTime,
    revoked_revision: u64,
    pcm: Option<D>,
    format: Option<CaptureFormat>,
    partial: Assembler,
    epoch: u64,
    sequence: u64,
    reset_required: Option<Discontinuity>,
    prepared: Option<PrepareReport>,
    running: bool,
}

impl<D: CaptureDevice> MicrophoneOn<D> {
    /// Validate only; no device handle is opened and no microphone samples are read.
    pub fn new(device: &str, boot: BootId, config: CaptureConfig) -> Result<Self> {
        if device.is_empty()
            || device.len() > 128
            || device
                .bytes()
                .any(|byte| byte == 0 || byte.is_ascii_control())
        {
            return Err(CaptureError::new(ErrorKind::InvalidConfig(
                "an explicit ALSA alias of 1..=128 non-control bytes is required",
            )));
        }
        let now = now();
        Ok(Self {
            device: device.into(),
            config,
            guard: BoundaryGuard::new(boot, now),
            last_now: now,
            revoked_revision: 0,
            pcm: None,
            format: None,
            partial: Assembler::default(),
            epoch: 0,
            sequence: 0,
            reset_required: None,
            prepared: None,
            running: false,
        })
    }

    /// Rejected stale/duplicate state cannot interrupt a newer valid capture.
    /// Installing Allowed state never opens the microphone implicitly.
    pub fn install(&mut self, snapshot: Snapshot) -> Result<StateUpdate> {
        let now = self.advance()?;
        let previous_generation = self.guard.state().map(Snapshot::microphone_generation);
        if let Err(error) = self.guard.install(now, snapshot) {
            if self.guard.is_faulted() {
                return Err(self.fail(
                    Discontinuity::ControlFault,
                    CaptureError::new(ErrorKind::Authority(error)),
                ));
            }
            self.maintain()?;
            return Err(CaptureError::new(ErrorKind::Authority(error)));
        }
        let privacy_changed =
            previous_generation.is_some_and(|old| old != snapshot.microphone_generation());
        if self.is_open()
            && (privacy_changed || snapshot.microphone_permission() != Permission::Allowed)
        {
            let discarded = self.partial.len;
            self.close(Discontinuity::PrivacyRevoked)?;
            return Ok(StateUpdate {
                partial_samples_discarded: discarded,
                closed: true,
            });
        }
        self.maintain()?;
        Ok(StateUpdate::default())
    }

    /// Explicitly open and start capture; fresh microphone permission is enough.
    /// Requiring listening_ready here would create a readiness dependency cycle.
    pub fn start(&mut self) -> Result<StartReport> {
        if !self.is_open() {
            self.prepare()?;
        }
        self.start_prepared()
    }

    /// Open/configure without starting acquisition or reading samples. This
    /// keeps potentially slow device setup outside the speaker/capture clock
    /// barrier. A prepared handle is explicitly not listening readiness.
    pub fn prepare(&mut self) -> Result<PrepareReport> {
        self.require_permission()?;
        if self.is_open() {
            return Err(CaptureError::new(ErrorKind::AlreadyOpen));
        }
        if let Some(reason) = self.reset_required {
            return Err(CaptureError::new(ErrorKind::ResetRequired(reason)));
        }
        let epoch = self.epoch.checked_add(1).ok_or_else(|| {
            self.fail(
                Discontinuity::CounterExhausted,
                CaptureError::new(ErrorKind::Discontinuity(Discontinuity::CounterExhausted)),
            )
        })?;
        let open_started_at_us = monotonic_us();
        self.pcm = match D::open(&self.device) {
            Ok(pcm) => Some(pcm),
            Err(error) => return Err(self.device_failed(error)),
        };
        let configured = self
            .pcm
            .as_mut()
            .expect("just opened")
            .configure(self.config);
        // Whatever the device reports is checked here as well: the bounds are
        // this boundary's, not the device's.
        let format = match configured {
            Ok(format) if valid_format(format, self.config.requested_frames()) => format,
            Ok(format) | Err(ConfigureError::Unsupported(format)) => {
                return Err(self.fail(
                    Discontinuity::DeviceError,
                    CaptureError::new(ErrorKind::UnsupportedFormat(format)),
                ));
            }
            Err(ConfigureError::Fault(error)) => {
                return Err(self.fail(Discontinuity::DeviceError, CaptureError::device(error)));
            }
        };
        self.format = Some(format);
        self.require_permission()?;
        self.epoch = epoch;
        self.sequence = 0;
        self.partial.clear();
        let report = PrepareReport {
            epoch,
            format,
            open_started_at_us,
            prepared_at_us: monotonic_us(),
            microphone_generation: self
                .guard
                .state()
                .expect("permission checked")
                .microphone_generation(),
        };
        self.prepared = Some(report);
        Ok(report)
    }

    /// Start an already configured microphone only after the speaker reference
    /// barrier. The first returned block is sequence 1; no warmup frames are read
    /// or discarded. Fresh permission is checked around the start syscall.
    pub fn start_prepared(&mut self) -> Result<StartReport> {
        self.require_permission()?;
        if self.running {
            return Err(CaptureError::new(ErrorKind::AlreadyOpen));
        }
        let prepared = self
            .prepared
            .ok_or_else(|| CaptureError::new(ErrorKind::Closed))?;
        let capture_started_at_us = monotonic_us();
        if let Err(error) = self
            .pcm
            .as_mut()
            .expect("permission preserved handle")
            .start()
        {
            return Err(self.device_failed(error));
        }
        self.require_permission()?;
        self.running = true;
        Ok(StartReport {
            epoch: prepared.epoch,
            format: prepared.format,
            open_started_at_us: prepared.open_started_at_us,
            prepared_at_us: prepared.prepared_at_us,
            capture_started_at_us,
            microphone_generation: self
                .guard
                .state()
                .expect("permission checked")
                .microphone_generation(),
            start_completed_at_us: monotonic_us(),
        })
    }

    /// At most one nonblocking read, no retry loop. None means partial/would-block.
    /// Process priority control first; do not cache permission outside this method.
    pub fn try_read(&mut self) -> Result<Option<CapturedFrame>> {
        self.maintain()?;
        self.require_permission()?;
        if !self.running {
            return Err(CaptureError::new(ErrorKind::NotStarted));
        }
        let Some(pcm) = self.pcm.as_mut() else {
            return Err(CaptureError::new(ErrorKind::Closed));
        };
        let read_started_at_us = monotonic_us();
        let result = pcm.read(self.partial.remaining_mut());
        let read_completed_at_us = monotonic_us();
        // A lease can expire during a syscall. Never return that audio.
        self.require_permission()?;
        let frames = match result {
            Ok(frames) => frames,
            Err(error) if error.would_block() => 0,
            Err(error) => return Err(self.device_failed(error)),
        };
        if read_completed_at_us.saturating_sub(read_started_at_us) > MAX_READ_US {
            return Err(self.discontinued(Discontinuity::ReadBudgetExceeded));
        }
        if frames == 0 {
            // The incomplete block keeps its original deadline across EAGAIN.
            self.maintain()?;
            return Ok(None);
        }
        if let Err(reason) = self
            .partial
            .accept(frames, read_started_at_us, read_completed_at_us)
        {
            return Err(self.discontinued(reason));
        }
        self.maintain()?;
        if self.partial.len != CAPTURE_SAMPLES {
            return Ok(None);
        }
        let status = match self
            .pcm
            .as_mut()
            .expect("maintain preserved handle")
            .status()
        {
            Ok(status) => status,
            Err(error) => return Err(self.device_failed(error)),
        };
        if let Some(reason) = state_discontinuity(status.state) {
            return Err(self.discontinued(reason));
        }
        let status_observed_at_us = monotonic_us();
        self.maintain()?;
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| self.discontinued(Discontinuity::CounterExhausted))?;
        let driver_time = status.monotonic_us.filter(|_| {
            self.format
                .is_some_and(|format| format.monotonic_status_timestamps)
        });
        let frame = CapturedFrame {
            samples: self.partial.samples,
            epoch: self.epoch,
            sequence,
            authority_generation: self.guard.state().expect("permission checked").generation(),
            microphone_generation: self
                .guard
                .state()
                .expect("permission checked")
                .microphone_generation(),
            timing: ReadTiming {
                first_read_started_at_us: self.partial.first_read_started_at_us,
                last_read_started_at_us: read_started_at_us,
                read_completed_at_us,
                successful_reads: self.partial.successful_reads,
                alsa_status_monotonic_us: driver_time,
                status_observed_at_us,
                available_frames: status.available,
                delayed_frames: status.delayed,
            },
        };
        self.partial.clear();
        self.sequence = sequence;
        Ok(Some(frame))
    }

    /// Call every worker tick (target <=10 ms), including while input is idle.
    pub fn maintain(&mut self) -> Result<()> {
        let now = self.advance()?;
        if !self.is_open() {
            return Ok(());
        }
        self.require_permission()?;
        if self.partial.expired(
            now.as_micros(),
            self.format.expect("open is configured").buffer_us(),
        ) {
            return Err(self.discontinued(Discontinuity::PartialTimeout));
        }
        Ok(())
    }

    /// Immediate local privacy action; no provider/network work and no drain.
    /// A newer snapshot and explicit reset/start are required before reopening.
    pub fn revoke(&mut self) -> Result<()> {
        if let Some(state) = self.guard.state() {
            self.revoked_revision = self.revoked_revision.max(state.revision());
        }
        self.close(Discontinuity::PrivacyRevoked)
    }

    /// A missing/broken trusted control stream is a permanent fault of this worker.
    pub fn control_lost(&mut self) -> Result<()> {
        self.guard.invalidate();
        self.close(Discontinuity::ControlFault)
    }

    /// Acknowledge discontinuity only after purging downstream queues/resetting AEC.
    /// Does not reopen or clear a control fault or local privacy revision floor.
    pub fn reset(&mut self) -> Result<()> {
        if self.is_open() {
            return Err(CaptureError::new(ErrorKind::AlreadyOpen));
        }
        if self.guard.is_faulted() {
            return Err(CaptureError::new(ErrorKind::Authority(
                lamp_turn_owner::Error::Faulted,
            )));
        }
        self.partial.clear();
        self.reset_required = None;
        Ok(())
    }

    pub fn is_open(&self) -> bool {
        self.pcm.is_some()
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn reset_required(&self) -> Option<Discontinuity> {
        self.reset_required
    }

    pub fn format(&self) -> Option<CaptureFormat> {
        self.format
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Samples the block being assembled still needs.
    pub fn missing(&self) -> usize {
        CAPTURE_SAMPLES - self.partial.len
    }

    /// Wait for the samples the block still needs or for one of `others`
    /// ([`CaptureDevice::wait`]); `None` unless the microphone is running on a device that can be
    /// waited on.
    pub fn wait(
        &mut self,
        others: &[std::os::fd::BorrowedFd<'_>],
        timeout: std::time::Duration,
    ) -> Option<std::result::Result<crate::device::Woken, crate::device::DeviceFault>> {
        if !self.running {
            return None;
        }
        let missing = self.missing();
        self.pcm.as_mut()?.wait(others, timeout, missing)
    }

    fn advance(&mut self) -> Result<MonoTime> {
        let now = now();
        if now < self.last_now {
            self.guard.invalidate();
            return Err(self.discontinued(Discontinuity::ClockRegression));
        }
        self.last_now = now;
        Ok(now)
    }

    fn require_permission(&mut self) -> Result<()> {
        let now = self.advance()?;
        if self.guard.is_faulted() {
            return Err(self.fail(
                Discontinuity::ControlFault,
                CaptureError::new(ErrorKind::Authority(lamp_turn_owner::Error::Faulted)),
            ));
        }
        let state = self.guard.state();
        let reason = match state {
            Some(state) if state.issued_at() <= now && now < state.expires_at() => {
                if state.microphone_permission() == Permission::Allowed
                    && state.revision() > self.revoked_revision
                {
                    return Ok(());
                }
                Discontinuity::PrivacyRevoked
            }
            Some(_) => Discontinuity::AuthorityExpired,
            None => Discontinuity::ControlFault,
        };
        if self.is_open() {
            Err(self.discontinued(reason))
        } else {
            Err(CaptureError::new(ErrorKind::NotPermitted))
        }
    }

    fn discontinued(&mut self, reason: Discontinuity) -> CaptureError {
        self.fail(reason, CaptureError::new(ErrorKind::Discontinuity(reason)))
    }

    fn device_failed(&mut self, error: DeviceFault) -> CaptureError {
        let reason = classify_device_error(error);
        self.fail(reason, CaptureError::device(error))
    }

    fn fail(&mut self, reason: Discontinuity, mut error: CaptureError) -> CaptureError {
        if let Err(close) = self.close(reason) {
            error.close_error = close.close_error;
        }
        error
    }

    fn close(&mut self, reason: Discontinuity) -> Result<()> {
        self.partial.clear();
        self.format = None;
        self.prepared = None;
        self.running = false;
        self.reset_required = Some(reason);
        // Taking the handle guarantees closure even when the discard fails.
        if let Some(mut pcm) = self.pcm.take()
            && let Err(error) = pcm.discard()
        {
            return Err(CaptureError {
                kind: ErrorKind::Discontinuity(reason),
                device_error: None,
                close_error: Some(error),
            });
        }
        Ok(())
    }
}

impl<D: CaptureDevice> Drop for MicrophoneOn<D> {
    fn drop(&mut self) {
        // Explicit revoke/control_lost returns discard errors. Destructor cleanup
        // cannot report them; the handle still closes when the device is dropped.
        self.partial.clear();
        if let Some(mut pcm) = self.pcm.take() {
            let _ = pcm.discard();
        }
    }
}

pub(crate) fn valid_format(format: CaptureFormat, requested_buffer: i64) -> bool {
    format.rate == CAPTURE_RATE
        && format.channels == 1
        && (1..=CAPTURE_SAMPLES as i64).contains(&format.period_frames)
        && (MIN_BUFFER_FRAMES..=MAX_BUFFER_FRAMES).contains(&format.buffer_frames)
        && format.buffer_frames <= requested_buffer
}

fn classify_device_error(error: DeviceFault) -> Discontinuity {
    match error.errno {
        errno::PIPE => Discontinuity::XRun,
        errno::STRPIPE => Discontinuity::Suspended,
        errno::NODEV | errno::BADFD => Discontinuity::Disconnected,
        _ => Discontinuity::DeviceError,
    }
}

fn state_discontinuity(state: DeviceState) -> Option<Discontinuity> {
    match state {
        DeviceState::Running => None,
        DeviceState::XRun => Some(Discontinuity::XRun),
        DeviceState::Suspended => Some(Discontinuity::Suspended),
        DeviceState::Disconnected => Some(Discontinuity::Disconnected),
        _ => Some(Discontinuity::DeviceError),
    }
}

fn now() -> MonoTime {
    MonoTime::from_micros(monotonic_us())
}

struct Assembler {
    samples: CaptureBlock,
    len: usize,
    first_read_started_at_us: u64,
    successful_reads: u16,
}

impl Default for Assembler {
    fn default() -> Self {
        Self {
            samples: [0; CAPTURE_SAMPLES],
            len: 0,
            first_read_started_at_us: 0,
            successful_reads: 0,
        }
    }
}

impl Assembler {
    fn remaining_mut(&mut self) -> &mut [i16] {
        &mut self.samples[self.len..]
    }

    fn accept(
        &mut self,
        frames: usize,
        started: u64,
        completed: u64,
    ) -> std::result::Result<(), Discontinuity> {
        if frames == 0 || frames > CAPTURE_SAMPLES - self.len {
            return Err(Discontinuity::InvalidRead);
        }
        if completed < started || (self.len > 0 && started < self.first_read_started_at_us) {
            return Err(Discontinuity::ClockRegression);
        }
        if self.len == 0 {
            self.first_read_started_at_us = started;
        }
        self.len += frames;
        self.successful_reads += 1; // At most 160 one-sample reads per block.
        Ok(())
    }

    fn expired(&self, now: u64, max_age_us: u64) -> bool {
        self.len > 0 && now.saturating_sub(self.first_read_started_at_us) >= max_age_us
    }

    fn clear(&mut self) -> usize {
        let discarded = self.len;
        self.samples.fill(0);
        self.len = 0;
        self.first_read_started_at_us = 0;
        self.successful_reads = 0;
        discarded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        device::timestamp_us,
        testing::{CaptureControl, ScriptedCapture},
    };
    use lamp_turn_owner::Controller;

    type Scripted = MicrophoneOn<ScriptedCapture>;

    /// A permitted microphone over a scripted device that holds no audio yet.
    fn permitted(name: &str) -> (Scripted, CaptureControl, Controller) {
        let device = CaptureControl::register(name);
        let boot = BootId::new([91; 16]).unwrap();
        let mut microphone = Scripted::new(name, boot, CaptureConfig::default()).unwrap();
        let mut controller = Controller::new(boot, now());
        controller
            .set_microphone_permission(now(), Permission::Allowed)
            .unwrap();
        microphone
            .install(controller.snapshot(now()).unwrap())
            .unwrap();
        (microphone, device, controller)
    }
    fn block(value: i16) -> [i16; CAPTURE_SAMPLES] {
        [value; CAPTURE_SAMPLES]
    }
    fn discontinuity(error: CaptureError) -> Discontinuity {
        match error.kind {
            ErrorKind::Discontinuity(reason) => reason,
            other => panic!("expected a discontinuity, got {other:?}"),
        }
    }

    #[test]
    fn partial_reads_preserve_sample_order_and_have_fixed_capacity() {
        for split in 1..CAPTURE_SAMPLES {
            let mut block = Assembler::default();
            block.remaining_mut()[..split].fill(11);
            block.accept(split, 100, 110).unwrap();
            block.remaining_mut().fill(22);
            block.accept(CAPTURE_SAMPLES - split, 200, 210).unwrap();
            assert_eq!(block.len, CAPTURE_SAMPLES);
            assert!(block.samples[..split].iter().all(|&value| value == 11));
            assert!(block.samples[split..].iter().all(|&value| value == 22));
            assert_eq!(block.first_read_started_at_us, 100);
            assert_eq!(block.successful_reads, 2);
            assert_eq!(block.clear(), CAPTURE_SAMPLES);
            assert_eq!(block.samples, [0; CAPTURE_SAMPLES]);
        }
    }

    #[test]
    fn partial_deadline_is_not_extended_by_more_samples() {
        let mut block = Assembler::default();
        block.accept(1, 100, 101).unwrap();
        block.accept(1, 19_000, 19_001).unwrap();
        assert!(!block.expired(20_099, 20_000));
        assert!(block.expired(20_100, 20_000));
        block.clear();
        assert!(!block.expired(1_000_000, 20_000));
        assert_eq!(block.accept(0, 0, 1), Err(Discontinuity::InvalidRead));
        assert_eq!(block.accept(161, 0, 1), Err(Discontinuity::InvalidRead));
        assert_eq!(block.accept(1, 2, 1), Err(Discontinuity::ClockRegression));
    }

    #[test]
    fn negotiation_rejects_large_or_incompatible_audio_buffers() {
        let good = CaptureFormat {
            rate: 16_000,
            channels: 1,
            period_frames: 160,
            buffer_frames: 640,
            monotonic_status_timestamps: false,
        };
        assert!(valid_format(good, 640));
        assert!(!valid_format(good, 320));
        for frames in [-1, 0, 1, 319, 641, i64::MAX] {
            assert!(!valid_format(
                CaptureFormat {
                    buffer_frames: frames,
                    ..good
                },
                640
            ));
        }
        for period in [-1, 0, 161, i64::MAX] {
            assert!(!valid_format(
                CaptureFormat {
                    period_frames: period,
                    ..good
                },
                640
            ));
        }
        assert!(!valid_format(
            CaptureFormat {
                rate: 48_000,
                ..good
            },
            640
        ));
        assert!(!valid_format(
            CaptureFormat {
                channels: 2,
                ..good
            },
            640
        ));
        for blocks in 0..=u8::MAX {
            assert_eq!(
                CaptureConfig::new(blocks).is_ok(),
                (2..=4).contains(&blocks)
            );
        }
    }

    #[test]
    fn status_and_timestamp_failures_are_not_fabricated_audio() {
        assert_eq!(
            state_discontinuity(DeviceState::XRun),
            Some(Discontinuity::XRun)
        );
        assert_eq!(
            state_discontinuity(DeviceState::Disconnected),
            Some(Discontinuity::Disconnected)
        );
        assert_eq!(
            state_discontinuity(DeviceState::Suspended),
            Some(Discontinuity::Suspended)
        );
        assert_eq!(state_discontinuity(DeviceState::Running), None);
        for (code, expected) in [
            (errno::PIPE, Discontinuity::XRun),
            (errno::STRPIPE, Discontinuity::Suspended),
            (errno::NODEV, Discontinuity::Disconnected),
            (errno::BADFD, Discontinuity::Disconnected),
            (errno::IO, Discontinuity::DeviceError),
        ] {
            assert_eq!(
                classify_device_error(DeviceFault::new("injected", code)),
                expected
            );
        }
        assert!(DeviceFault::new("read", errno::AGAIN).would_block());
        assert!(!DeviceFault::new("read", errno::PIPE).would_block());
        assert_eq!(timestamp_us(5, 120_000), Some(5_000_120));
        for (seconds, nanos) in [(0, 0), (-1, 2), (1, -1), (1, 1_000_000_000), (i64::MAX, 1)] {
            assert_eq!(timestamp_us(seconds, nanos), None);
        }
    }

    #[test]
    fn blocks_are_assembled_across_short_reads_in_order_from_sequence_one() {
        let (mut microphone, device, _controller) = permitted("scripted:assemble");
        let started = microphone.start().unwrap();
        assert_eq!((started.epoch, device.counts().0), (1, 1));
        // Nothing captured yet is not an error and not a frame.
        assert!(microphone.try_read().unwrap().is_none());
        device.push(&[7; 100]);
        assert!(microphone.try_read().unwrap().is_none(), "100 of 160");
        device.push(&[8; 60]);
        device.push(&block(9));
        let first = microphone.try_read().unwrap().expect("first block");
        assert_eq!((first.epoch, first.sequence), (1, 1));
        assert!(first.samples[..100].iter().all(|&sample| sample == 7));
        assert!(first.samples[100..].iter().all(|&sample| sample == 8));
        assert_eq!(first.timing.successful_reads, 2);
        // The scripted device has no status clock, so none is reported.
        assert_eq!(first.timing.alsa_status_monotonic_us, None);
        let second = microphone.try_read().unwrap().expect("second block");
        assert_eq!(second.sequence, 2);
        assert_eq!(second.samples, block(9));
        assert!(microphone.try_read().unwrap().is_none());
        microphone.revoke().unwrap();
        assert_eq!(device.counts().2, 1, "revocation discards the device");
    }

    #[test]
    fn an_xrun_closes_the_device_destroys_the_partial_block_and_restarts_as_a_new_epoch() {
        let (mut microphone, device, mut controller) = permitted("scripted:xrun");
        microphone.start().unwrap();
        device.push(&[300; 80]);
        assert!(microphone.try_read().unwrap().is_none());
        device.push(&[301; 400]);
        device.fail_next("read", errno::PIPE);
        let error = microphone.try_read().unwrap_err();
        assert_eq!(
            error.device_error,
            Some(DeviceFault::new("read", errno::PIPE))
        );
        assert_eq!(discontinuity(error), Discontinuity::XRun);
        assert!(!microphone.is_open() && !microphone.is_running());
        assert_eq!(microphone.partial.len, 0);
        assert_eq!(microphone.partial.samples, [0; CAPTURE_SAMPLES]);
        assert_eq!(microphone.reset_required(), Some(Discontinuity::XRun));
        // Nothing reopens by itself, and not before the caller acknowledges.
        assert!(matches!(
            microphone.start().unwrap_err().kind,
            ErrorKind::ResetRequired(Discontinuity::XRun)
        ));
        assert_eq!(device.counts().0, 1);
        microphone.reset().unwrap();
        microphone
            .install(controller.snapshot(now()).unwrap())
            .unwrap();
        let restarted = microphone.start().unwrap();
        assert_eq!(restarted.epoch, 2);
        // What the old handle still held went with it.
        assert_eq!(device.queued(), 0);
        device.push(&block(302));
        let frame = microphone.try_read().unwrap().unwrap();
        assert_eq!((frame.epoch, frame.sequence), (2, 1));
        assert_eq!(frame.samples, block(302));
    }

    /// A microphone a stall closed opens again only under a fresh authority
    /// that still allows it: muted in the meantime, it stays closed (#784).
    #[test]
    fn after_a_stall_a_muted_authority_keeps_the_microphone_closed() {
        let (mut microphone, device, mut controller) = permitted("scripted:stall-mute");
        microphone.start().unwrap();
        device.push(&[301; 400]);
        device.fail_next("read", errno::PIPE);
        let error = microphone.try_read().unwrap_err();
        assert_eq!(discontinuity(error), Discontinuity::XRun);
        microphone.reset().unwrap();
        controller
            .set_microphone_permission(now(), Permission::Denied)
            .unwrap();
        microphone
            .install(controller.snapshot(now()).unwrap())
            .unwrap();
        assert!(matches!(
            microphone.prepare().unwrap_err().kind,
            ErrorKind::NotPermitted
        ));
        assert_eq!(device.counts().0, 1, "not opened again while muted");
    }

    #[test]
    fn a_device_that_vanishes_or_stops_running_is_a_discontinuity_never_silence() {
        for (index, (state, expected)) in [
            (DeviceState::XRun, Discontinuity::XRun),
            (DeviceState::Suspended, Discontinuity::Suspended),
            (DeviceState::Disconnected, Discontinuity::Disconnected),
            (DeviceState::Setup, Discontinuity::DeviceError),
        ]
        .into_iter()
        .enumerate()
        {
            let (mut microphone, device, _controller) =
                permitted(&format!("scripted:state-{index}"));
            microphone.start().unwrap();
            device.push(&block(5));
            // The block was read whole, and the device then says it is not
            // running: that block is not handed over.
            device.set_state(state);
            assert_eq!(discontinuity(microphone.try_read().unwrap_err()), expected);
            assert!(!microphone.is_open());
        }
        for (index, (code, expected)) in [
            (errno::NODEV, Discontinuity::Disconnected),
            (errno::BADFD, Discontinuity::Disconnected),
            (errno::STRPIPE, Discontinuity::Suspended),
            (errno::IO, Discontinuity::DeviceError),
        ]
        .into_iter()
        .enumerate()
        {
            let (mut microphone, device, _controller) =
                permitted(&format!("scripted:errno-{index}"));
            microphone.start().unwrap();
            device.fail_next("read", code);
            assert_eq!(discontinuity(microphone.try_read().unwrap_err()), expected);
            assert_eq!(microphone.reset_required(), Some(expected));
        }
        // The status call itself failing, after a whole block was read.
        let (mut microphone, device, _controller) = permitted("scripted:status");
        microphone.start().unwrap();
        device.push(&block(5));
        device.fail_next("status", errno::NODEV);
        assert_eq!(
            discontinuity(microphone.try_read().unwrap_err()),
            Discontinuity::Disconnected
        );
    }

    #[test]
    fn a_read_that_blocks_past_its_budget_is_a_discontinuity_not_late_audio() {
        let (mut microphone, device, _controller) = permitted("scripted:stall");
        microphone.start().unwrap();
        device.push(&block(5));
        device.stall_next_read(std::time::Duration::from_micros(MAX_READ_US + 5_000));
        let before = monotonic_us();
        assert_eq!(
            discontinuity(microphone.try_read().unwrap_err()),
            Discontinuity::ReadBudgetExceeded
        );
        assert!(monotonic_us() - before >= MAX_READ_US);
        // The block that the stuck call eventually returned is not delivered.
        assert!(!microphone.is_open());
        assert_eq!(microphone.partial.len, 0);
        assert!(matches!(
            microphone.try_read().unwrap_err().kind,
            ErrorKind::NotStarted | ErrorKind::Closed
        ));
    }

    #[test]
    fn a_block_left_incomplete_for_a_buffer_length_is_dropped_not_finished_late() {
        let (mut microphone, device, _controller) = permitted("scripted:partial");
        let started = microphone.start().unwrap();
        device.push(&[5; 10]);
        assert!(microphone.try_read().unwrap().is_none());
        // 640 frames at 16 kHz: forty milliseconds.
        std::thread::sleep(std::time::Duration::from_micros(
            started.format.buffer_frames as u64 * 1_000_000 / 16_000 + 2_000,
        ));
        device.push(&[6; 150]);
        assert_eq!(
            discontinuity(microphone.try_read().unwrap_err()),
            Discontinuity::PartialTimeout
        );
        assert!(!microphone.is_open());
    }

    #[test]
    fn open_and_configuration_failures_leave_nothing_open() {
        let boot = BootId::new([92; 16]).unwrap();
        let mut controller = Controller::new(boot, now());
        controller
            .set_microphone_permission(now(), Permission::Allowed)
            .unwrap();
        // A name no device answers to.
        let mut absent = Scripted::new("scripted:absent", boot, CaptureConfig::default()).unwrap();
        absent.install(controller.snapshot(now()).unwrap()).unwrap();
        let error = absent.start().unwrap_err();
        assert_eq!(
            error.device_error.map(|fault| fault.errno),
            Some(errno::NOENT)
        );
        assert!(!absent.is_open());

        let (mut microphone, device, _controller) = permitted("scripted:configure");
        device.fail_next("configure", errno::IO);
        let error = microphone.prepare().unwrap_err();
        assert_eq!(
            error.device_error,
            Some(DeviceFault::new("configure", errno::IO))
        );
        assert!(!microphone.is_open());
        assert_eq!(device.counts().2, 1, "the opened handle was discarded");
        assert_eq!(
            microphone.reset_required(),
            Some(Discontinuity::DeviceError)
        );

        // A device that configures to something outside the accepted bounds.
        for (index, change) in [
            (|format: &mut CaptureFormat| format.rate = 48_000) as fn(&mut CaptureFormat),
            |format| format.channels = 2,
            |format| format.buffer_frames = 4_096,
            |format| format.period_frames = 0,
        ]
        .into_iter()
        .enumerate()
        {
            let (mut microphone, device, _controller) =
                permitted(&format!("scripted:format-{index}"));
            let mut format = CaptureFormat {
                rate: 16_000,
                channels: 1,
                period_frames: 160,
                buffer_frames: 640,
                monotonic_status_timestamps: false,
            };
            change(&mut format);
            device.set_format(format);
            assert_eq!(
                microphone.start().unwrap_err().kind,
                ErrorKind::UnsupportedFormat(format)
            );
            assert!(!microphone.is_open());
        }
    }

    #[test]
    fn privacy_lineage_change_discards_an_incomplete_block() {
        let (mut microphone, device, mut controller) = permitted("scripted:privacy");
        microphone.start().unwrap();
        device.push(&[400; 40]);
        assert!(microphone.try_read().unwrap().is_none());
        // Denied and allowed again inside one snapshot interval: the
        // generation moved, so the handle closes anyway.
        controller
            .set_microphone_permission(now(), Permission::Denied)
            .unwrap();
        controller
            .set_microphone_permission(now(), Permission::Allowed)
            .unwrap();
        let update = microphone
            .install(controller.snapshot(now()).unwrap())
            .unwrap();
        assert_eq!(update.partial_samples_discarded, 40);
        assert!(update.closed);
        assert_eq!(microphone.partial.samples, [0; CAPTURE_SAMPLES]);
        assert_eq!(device.counts().2, 1);
    }

    #[test]
    fn a_failed_discard_is_reported_and_the_handle_is_still_gone() {
        let (mut microphone, device, _controller) = permitted("scripted:discard");
        microphone.start().unwrap();
        device.fail_next("discard", errno::IO);
        let error = microphone.revoke().unwrap_err();
        assert_eq!(
            error.close_error,
            Some(DeviceFault::new("discard", errno::IO))
        );
        assert!(!microphone.is_open());
        assert_eq!(
            microphone.reset_required(),
            Some(Discontinuity::PrivacyRevoked)
        );
    }
}
