//! Lamp's 32-pixel WS2812 codec and one guarded output writer.
//!
//! The driver accepts conversational light permits, plus explicit black frames.
//! It does not choose animations, privacy colors, listening cues, or readiness,
//! with three exceptions that need no permit: the asleep face ([`asleep`], #712),
//! which says only that the lamp cannot hear, the setup light ([`setup`],
//! #806), shown only while the lamp is not set up and no voice runs, and the
//! waking face ([`waking`], #1681), shown only from power-on until the voice
//! takes the ring.
//! Its separate physical-mute indication accepts only fresh denied authority and
//! a matching physical observation; that path can render only a dim red breath.
//! An owning worker must drain priority control messages before calling `show`
//! and call `tick` while a cue is displayed so leases expire without new output.

use lamp_turn_owner::{BootId, BoundaryGuard, MonoTime, OutputKind, OutputPermit, Snapshot};
use std::{fmt, io};

pub mod asleep;
mod privacy;
pub mod setup;
pub mod waking;
pub use privacy::PrivacyObservation;

#[cfg(target_os = "linux")]
pub mod linux;

pub const PIXEL_COUNT: usize = 32;
pub const PRIMER_BYTES: usize = 10;
pub const RESET_BYTES: usize = 250;
pub const FRAME_BYTES: usize = PRIMER_BYTES + PIXEL_COUNT * 24 + RESET_BYTES;
pub const SPI_SPEED_HZ: u32 = 6_400_000;
/// An observed-return budget, not a timeout capable of interrupting a kernel call.
pub const WRITE_BUDGET_US: u64 = 10_000;
/// Writes over the budget are a run of late frames, not a fault, until the
/// run has lasted this long. A run is late writes in a row: an on-time
/// write, or more than [`LATE_GAP_US`] with no write, ends it. The frame of a late write did go out. On
/// lamp-4ace the SD/MMC host re-tunes about once an hour and holds the bus
/// for 0.3 s; one write then returns after 11 to 15 ms (#911).
pub const LATE_RUN_US: u64 = 2_000_000;
/// The longest pause between two late writes that still keeps them in one run:
/// one frame period of the slowest steady writer (a frame older than this is
/// dropped by the voice's ring worker anyway). A held cue or a dark ring writes
/// nothing, so a late write after it starts a new run.
pub const LATE_GAP_US: u64 = 100_000;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Rgb {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Rgb {
    pub const BLACK: Self = Self::new(0, 0, 0);

    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }

    /// Scale all channels together; integer quantization rounds to nearest.
    pub fn capped(self, ceiling: ChannelCeiling) -> Self {
        let peak = self.red.max(self.green).max(self.blue);
        if peak <= ceiling.0 {
            return self;
        }
        let scale = |channel: u8| {
            ((u16::from(channel) * u16::from(ceiling.0) + u16::from(peak) / 2) / u16::from(peak))
                as u8
        };
        Self::new(scale(self.red), scale(self.green), scale(self.blue))
    }
}

/// The policy supplies the current ceiling (for example 40 at night).
/// Zero is valid; values above the inherited maximum of 120 are rejected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChannelCeiling(u8);

impl ChannelCeiling {
    pub fn new(value: u16) -> Result<Self, InvalidCeiling> {
        if value > 120 {
            return Err(InvalidCeiling(value));
        }
        Ok(Self(value as u8))
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidCeiling(pub u16);

impl fmt::Display for InvalidCeiling {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ring channel ceiling {} is outside 0..=120", self.0)
    }
}

impl std::error::Error for InvalidCeiling {}

/// Fixed-size wire data. There is no partial-frame or arbitrary-length API.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncodedFrame([u8; FRAME_BYTES]);

impl EncodedFrame {
    pub fn bytes(&self) -> &[u8; FRAME_BYTES] {
        &self.0
    }

    pub fn black() -> Self {
        encode(&[Rgb::BLACK; PIXEL_COUNT], ChannelCeiling(0))
    }
}

fn encode_byte(byte: u8) -> [u8; 8] {
    std::array::from_fn(|bit| {
        if byte & (0x80 >> bit) == 0 {
            0xC0
        } else {
            0xFC
        }
    })
}

pub fn encode(pixels: &[Rgb; PIXEL_COUNT], ceiling: ChannelCeiling) -> EncodedFrame {
    let mut frame = [0; FRAME_BYTES];
    for (index, color) in pixels.iter().copied().enumerate() {
        let color = color.capped(ceiling);
        let offset = PRIMER_BYTES + index * 24;
        for (channel, value) in [color.green, color.red, color.blue].into_iter().enumerate() {
            frame[offset + channel * 8..offset + (channel + 1) * 8]
                .copy_from_slice(&encode_byte(value));
        }
    }
    EncodedFrame(frame)
}

/// A sink must send one complete frame with one bounded-size operation.
/// It must report short writes, and must not retain/replay a frame asynchronously.
/// This interface does not promise a hard wall-clock bound on kernel I/O.
pub trait FrameSink {
    fn write_frame(&mut self, frame: &EncodedFrame) -> io::Result<()>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WriteReceipt {
    started_at: MonoTime,
    finished_at: MonoTime,
}

impl WriteReceipt {
    pub fn started_at(self) -> MonoTime {
        self.started_at
    }

    pub fn finished_at(self) -> MonoTime {
        self.finished_at
    }

    pub fn elapsed_us(self) -> u64 {
        self.finished_at.as_micros() - self.started_at.as_micros()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlankReason {
    Startup,
    ExplicitOff,
    Shutdown,
    ControlLost,
    AuthorityLost(lamp_turn_owner::Error),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlankReceipt {
    pub reason: BlankReason,
    /// Successful transfer only, never an assertion that the LEDs are optically off.
    pub write: WriteReceipt,
}

#[derive(Debug)]
pub enum WriteFailure {
    Io(io::Error),
    ClockRegression,
    OverBudget { elapsed_us: u64 },
}

impl fmt::Display for WriteFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "SPI frame write failed: {error}"),
            Self::ClockRegression => f.write_str("monotonic clock regressed during SPI write"),
            Self::OverBudget { elapsed_us } => write!(
                f,
                "SPI write returned after {elapsed_us} us; budget is {WRITE_BUDGET_US} us"
            ),
        }
    }
}

impl std::error::Error for WriteFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum Failure {
    Authority(lamp_turn_owner::Error),
    Write(WriteFailure),
    Faulted,
    Stopped,
}

#[derive(Debug)]
pub struct Error {
    pub cause: Failure,
    /// At most one cleanup frame is attempted. Its failure is not hidden by the
    /// original error; there is no unbounded retry and no I/O inside Drop.
    pub cleanup_error: Option<WriteFailure>,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.cause {
            Failure::Authority(error) => write!(f, "ring authority rejected: {error}")?,
            Failure::Write(error) => write!(f, "ring output fault: {error}")?,
            Failure::Faulted => {
                f.write_str("ring writer is faulted; supervised restart required")?
            }
            Failure::Stopped => f.write_str("ring writer has stopped")?,
        }
        if let Some(error) = &self.cleanup_error {
            write!(f, "; cleanup black frame also failed: {error}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.cause {
            Failure::Authority(error) => Some(error),
            Failure::Write(error) => Some(error),
            Failure::Faulted | Failure::Stopped => None,
        }
    }
}

fn now() -> MonoTime {
    MonoTime::from_micros(lamp_ipc::monotonic_us())
}

/// Own this in the sole ring process. State starts disarmed and a startup black
/// frame is sent. No cue remains authorized merely because it was once written.
///
/// `tick` is required even when no output messages arrive. The owning event loop
/// should call it at least every 10 ms and prioritize control/revocation packets.
/// A crashed worker cannot blank the ring itself; the supervisor must arrange an
/// exclusive replacement writer. Rejected old messages never acquire ownership.
pub struct GuardedRing<S: FrameSink> {
    sink: S,
    guard: BoundaryGuard,
    active: Option<OutputPermit>,
    /// A resting frame is on the strip: one that belongs to no turn.
    resting: bool,
    /// The separate physical-mute indication, never a conversational permit.
    privacy: Option<PrivacyObservation>,
    privacy_painted_at: Option<MonoTime>,
    faulted: bool,
    stopped: bool,
    /// When the current run of late writes began.
    late_since: Option<MonoTime>,
    /// When the previous write returned, if it was late.
    late_last: Option<MonoTime>,
    /// Late writes so far that were kept as frames, for the owner to say.
    late_writes: u64,
    clock: fn() -> MonoTime,
    black: EncodedFrame,
}

impl<S: FrameSink> GuardedRing<S> {
    pub fn new(sink: S, controller_boot: BootId) -> Result<Self, Error> {
        Self::with_clock(sink, controller_boot, now)
    }

    fn with_clock(
        sink: S,
        controller_boot: BootId,
        clock: fn() -> MonoTime,
    ) -> Result<Self, Error> {
        let mut ring = Self {
            sink,
            guard: BoundaryGuard::new(controller_boot, clock()),
            active: None,
            resting: false,
            privacy: None,
            privacy_painted_at: None,
            faulted: false,
            stopped: false,
            late_since: None,
            late_last: None,
            late_writes: 0,
            clock,
            black: EncodedFrame::black(),
        };
        ring.blank(BlankReason::Startup).map_err(|failure| Error {
            cause: Failure::Write(failure),
            cleanup_error: None,
        })?;
        Ok(ring)
    }

    /// Install only a trusted supervisor/controller message. A stale or duplicate
    /// snapshot is rejected while a newer valid displayed cue remains unchanged.
    /// A valid revocation blanks the old cue immediately, without a new paint.
    pub fn install_authority(&mut self, snapshot: Snapshot) -> Result<Option<BlankReceipt>, Error> {
        self.ensure_live()?;
        if let Err(reason) = self.guard.install((self.clock)(), snapshot) {
            return Err(self.reject(reason));
        }
        self.tick()
    }

    /// Prepare pixels first, then check a fresh shared monotonic time immediately
    /// before the single frame write. A receipt is not optical verification.
    pub fn show(
        &mut self,
        pixels: &[Rgb; PIXEL_COUNT],
        ceiling: ChannelCeiling,
        permit: OutputPermit,
    ) -> Result<WriteReceipt, Error> {
        self.ensure_live()?;
        let frame = encode(pixels, ceiling);
        let started_at = (self.clock)();
        if let Err(reason) = self.guard.check(started_at, permit, OutputKind::Light) {
            return Err(self.reject(reason));
        }
        let receipt = match self.transfer(&frame, started_at) {
            Ok(receipt) => receipt,
            Err(failure) => return Err(self.write_fault(failure)),
        };
        self.active = Some(permit);
        self.resting = false;
        self.privacy = None;
        self.privacy_painted_at = None;
        // A lease may expire while the kernel write is in flight. Do not leave
        // that completed but now unauthorized cue latched on the strip.
        if let Err(reason) = self.guard.check((self.clock)(), permit, OutputKind::Light) {
            return Err(self.reject(reason));
        }
        Ok(receipt)
    }

    /// Paint a resting frame: one that belongs to no turn and claims nothing
    /// about one. It says only that the installed authority is fresh, the
    /// microphone is permitted and input is available, so it is refused, with
    /// no write, unless all of that holds and no turn owns the lamp. It never
    /// replaces a displayed cue. `tick` blanks it when that stops holding; an
    /// owner appearing does not, so a cue can be painted over it with no dark
    /// frame between. `None`: not painted.
    pub fn show_resting(
        &mut self,
        pixels: &[Rgb; PIXEL_COUNT],
        ceiling: ChannelCeiling,
    ) -> Result<Option<WriteReceipt>, Error> {
        self.ensure_live()?;
        let started_at = (self.clock)();
        if self.active.is_some() || self.resting_denial(started_at, true).is_some() {
            return Ok(None);
        }
        let frame = encode(pixels, ceiling);
        match self.transfer(&frame, started_at) {
            Ok(receipt) => {
                self.resting = true;
                self.privacy = None;
                self.privacy_painted_at = None;
                Ok(Some(receipt))
            }
            Err(failure) => Err(self.write_fault(failure)),
        }
    }

    /// Blank the displayed cue when its own permit expires or is revoked. An
    /// unrelated rejected old message cannot clear a newer valid presentation.
    pub fn tick(&mut self) -> Result<Option<BlankReceipt>, Error> {
        self.ensure_live()?;
        self.reconcile().map_err(|failure| {
            self.latch_fault();
            Error {
                cause: Failure::Write(failure),
                cleanup_error: None,
            }
        })
    }

    /// An explicit black frame is not a privacy or readiness indicator and does
    /// not create authority. The controller still owns cancellation/permissions.
    /// May be used for one explicit cleanup retry after a reported writer fault.
    pub fn off(&mut self) -> Result<BlankReceipt, Error> {
        self.blank(BlankReason::ExplicitOff).map_err(|failure| {
            self.latch_fault();
            Error {
                cause: Failure::Write(failure),
                cleanup_error: None,
            }
        })
    }

    /// Call for trusted control decode failure, transport loss or worker fault.
    /// Once faulted, later packets cannot revive this writer. Only the first call
    /// attempts cleanup; a supervisor decides whether a restart is safe.
    pub fn control_lost(&mut self) -> Result<BlankReceipt, Error> {
        self.ensure_live()?;
        self.latch_fault();
        self.blank(BlankReason::ControlLost)
            .map_err(|failure| Error {
                cause: Failure::Write(failure),
                cleanup_error: None,
            })
    }

    /// Blank now because a successor to the installed authority came past its
    /// time (#1178): the owner paints nothing until a fresher one installs.
    /// Not a fault, so that one can install.
    pub fn lapse(&mut self) -> Result<BlankReceipt, Error> {
        self.ensure_live()?;
        self.blank(BlankReason::AuthorityLost(
            lamp_turn_owner::Error::ExpiredState,
        ))
        .map_err(|failure| {
            self.latch_fault();
            Error {
                cause: Failure::Write(failure),
                cleanup_error: None,
            }
        })
    }

    /// Explicit bounded-count cleanup. Drop intentionally performs no I/O.
    pub fn shutdown(&mut self) -> Result<Option<BlankReceipt>, Error> {
        if self.stopped {
            return Ok(None);
        }
        self.stopped = true;
        self.guard.invalidate();
        self.blank(BlankReason::Shutdown)
            .map(Some)
            .map_err(|failure| Error {
                cause: Failure::Write(failure),
                cleanup_error: None,
            })
    }

    pub fn is_faulted(&self) -> bool {
        self.faulted || (!self.stopped && self.guard.is_faulted())
    }

    /// Late writes so far that were kept as frames (see [`LATE_RUN_US`]).
    pub fn late_writes(&self) -> u64 {
        self.late_writes
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped
    }

    fn ensure_live(&self) -> Result<(), Error> {
        let cause = if self.stopped {
            Some(Failure::Stopped)
        } else if self.faulted {
            Some(Failure::Faulted)
        } else {
            None
        };
        cause.map_or(Ok(()), |cause| {
            Err(Error {
                cause,
                cleanup_error: None,
            })
        })
    }

    fn transfer(
        &mut self,
        frame: &EncodedFrame,
        started_at: MonoTime,
    ) -> Result<WriteReceipt, WriteFailure> {
        self.sink.write_frame(frame).map_err(WriteFailure::Io)?;
        let finished_at = (self.clock)();
        let elapsed_us = finished_at
            .as_micros()
            .checked_sub(started_at.as_micros())
            .ok_or(WriteFailure::ClockRegression)?;
        if elapsed_us > WRITE_BUDGET_US {
            let in_a_row = self.late_last.is_some_and(|last| {
                started_at.as_micros().saturating_sub(last.as_micros()) <= LATE_GAP_US
            });
            if !in_a_row {
                self.late_since = Some(started_at);
            }
            self.late_last = Some(finished_at);
            let since = self.late_since.unwrap_or(started_at);
            if finished_at.as_micros().saturating_sub(since.as_micros()) >= LATE_RUN_US {
                return Err(WriteFailure::OverBudget { elapsed_us });
            }
            self.late_writes += 1;
        } else {
            self.late_since = None;
            self.late_last = None;
        }
        Ok(WriteReceipt {
            started_at,
            finished_at,
        })
    }

    /// Why a resting frame may not be on the strip now. Painting a new one
    /// also needs the lamp to have no owner; keeping the last one does not.
    fn resting_denial(&self, now: MonoTime, painting: bool) -> Option<lamp_turn_owner::Error> {
        use lamp_turn_owner::{Error as Denial, Permission};
        if self.guard.is_faulted() {
            return Some(Denial::Faulted);
        }
        let Some(state) = self.guard.state() else {
            return Some(Denial::NoAuthority);
        };
        if state.issued_at() > now {
            Some(Denial::FutureState)
        } else if now >= state.expires_at() {
            Some(Denial::ExpiredState)
        } else if state.microphone_permission() != Permission::Allowed {
            Some(Denial::PrivacyClosed)
        } else if !state.listening_ready(now) {
            Some(Denial::InputUnavailable)
        } else if painting && state.owner().is_some() {
            Some(Denial::StaleOwner)
        } else {
            None
        }
    }

    fn blank(&mut self, reason: BlankReason) -> Result<BlankReceipt, WriteFailure> {
        self.active = None;
        self.resting = false;
        self.privacy = None;
        self.privacy_painted_at = None;
        // Copy a fixed 1,028-byte frame; no heap allocation or arbitrary retry.
        let frame = self.black.clone();
        let write = self.transfer(&frame, (self.clock)())?;
        Ok(BlankReceipt { reason, write })
    }

    fn latch_fault(&mut self) {
        self.faulted = true;
        self.guard.invalidate();
    }

    fn write_fault(&mut self, failure: WriteFailure) -> Error {
        self.latch_fault();
        let cleanup_error = self
            .blank(BlankReason::AuthorityLost(lamp_turn_owner::Error::Faulted))
            .err();
        Error {
            cause: Failure::Write(failure),
            cleanup_error,
        }
    }

    fn reconcile(&mut self) -> Result<Option<BlankReceipt>, WriteFailure> {
        let reason = if self.guard.is_faulted() {
            self.faulted = true;
            Some(lamp_turn_owner::Error::Faulted)
        } else if let Some(permit) = self.active {
            self.guard
                .check((self.clock)(), permit, OutputKind::Light)
                .err()
        } else if self.resting {
            self.resting_denial((self.clock)(), false)
        } else if let Some(observation) = self.privacy {
            self.privacy_denial(observation, (self.clock)())
        } else {
            None
        };
        if reason == Some(lamp_turn_owner::Error::ClockRegression) {
            self.latch_fault();
        } else if self.guard.is_faulted() {
            self.faulted = true;
        }
        reason
            .map(|reason| self.blank(BlankReason::AuthorityLost(reason)))
            .transpose()
    }

    fn reject(&mut self, reason: lamp_turn_owner::Error) -> Error {
        let cleanup_error = self.reconcile().err();
        if cleanup_error.is_some() {
            self.latch_fault();
        }
        Error {
            cause: Failure::Authority(reason),
            cleanup_error,
        }
    }
}

#[cfg(any(target_os = "linux", test))]
fn write_once(writer: &mut impl io::Write, frame: &EncodedFrame) -> io::Result<()> {
    let written = writer.write(frame.bytes())?;
    if written != FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::WriteZero,
            format!("short SPI frame: {written} of {FRAME_BYTES} bytes"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
