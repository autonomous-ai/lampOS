//! The device calls the microphone and speaker boundaries make, as traits.
//!
//! Everything that decides whether audio may be read or written lives above
//! these traits and is the same whatever is underneath: permission, privacy
//! generations, ownership of queued playback, epochs, sequence numbers,
//! deadlines and what counts as a discontinuity. Below them is one call per
//! method and no policy. The ALSA implementations call exactly what the
//! boundaries called before the traits existed.
//!
//! The point is that the same boundaries, and the worker loops built on
//! them, can run over a scripted device: a stalled read, an xrun, a device
//! that vanishes or a speaker that stops draining can then be produced on
//! demand, off the board. A scripted device proves the logic above the trait.
//! It says nothing about a real codec, clock or room.
use crate::capture::{CaptureConfig, CaptureFormat};
use std::{fmt, os::fd::BorrowedFd, time::Duration};

#[cfg(target_os = "linux")]
pub use crate::alsa_device::AlsaCapture;

/// The call a failed open names: ALSA's own, so that a scripted device and
/// the lamp's report it alike, and a check written against one holds on the
/// other.
pub const OPEN_CALL: &str = "snd_pcm_open";

/// The `errno` values the boundaries act on, by name. Two of them exist only
/// on Linux, where ALSA returns them. Elsewhere they keep their Linux numbers,
/// so that a scripted device can still produce them.
pub mod errno {
    use rustix::io::Errno;
    pub const AGAIN: i32 = Errno::AGAIN.raw_os_error();
    /// Another handle still has the device.
    pub const BUSY: i32 = Errno::BUSY.raw_os_error();
    pub const IO: i32 = Errno::IO.raw_os_error();
    pub const NODEV: i32 = Errno::NODEV.raw_os_error();
    pub const NOENT: i32 = Errno::NOENT.raw_os_error();
    pub const PIPE: i32 = Errno::PIPE.raw_os_error();
    /// The stream is suspended.
    #[cfg(target_os = "linux")]
    pub const STRPIPE: i32 = Errno::STRPIPE.raw_os_error();
    #[cfg(not(target_os = "linux"))]
    pub const STRPIPE: i32 = 86;
    /// The handle is in a state the call cannot be made in.
    #[cfg(target_os = "linux")]
    pub const BADFD: i32 = Errno::BADFD.raw_os_error();
    #[cfg(not(target_os = "linux"))]
    pub const BADFD: i32 = 77;
}

/// One failed device call: which call, and the positive `errno` it returned.
/// No driver message is kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceFault {
    pub errno: i32,
    pub call: &'static str,
}
impl DeviceFault {
    pub fn new(call: &'static str, errno: i32) -> Self {
        Self { errno, call }
    }
    /// A nonblocking call that had nothing to do yet.
    pub fn would_block(self) -> bool {
        std::io::Error::from_raw_os_error(self.errno).kind() == std::io::ErrorKind::WouldBlock
    }
}
impl fmt::Display for DeviceFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} failed with errno {}", self.call, self.errno)
    }
}
impl std::error::Error for DeviceFault {}

/// The stream state a device reports, reduced to what the boundaries act on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceState {
    Setup,
    Prepared,
    Running,
    XRun,
    Draining,
    Suspended,
    Disconnected,
    /// Open, paused or anything a driver adds later.
    Other,
}

/// One observation taken after a block has been read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureStatus {
    pub state: DeviceState,
    /// The driver's status clock, when it has a valid monotonic one. Not the
    /// time a sample was acquired.
    pub monotonic_us: Option<u64>,
    pub available: i64,
    pub delayed: i64,
}

/// Why a capture device could not be configured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigureError {
    Fault(DeviceFault),
    /// It was configured, to something outside the accepted bounds.
    Unsupported(CaptureFormat),
}

/// A driver status time in microseconds, or `None` when the driver gave no
/// usable one: negative, out of range, or the all-zero "not set" value.
pub fn timestamp_us(seconds: i64, nanos: i64) -> Option<u64> {
    if seconds < 0 || !(0..1_000_000_000).contains(&nanos) || (seconds == 0 && nanos == 0) {
        return None;
    }
    u64::try_from(seconds)
        .ok()?
        .checked_mul(1_000_000)?
        .checked_add(nanos as u64 / 1_000)
}

pub trait CaptureDevice: Sized {
    /// Open nonblocking. Nothing is configured, started or read.
    fn open(name: &str) -> Result<Self, DeviceFault>;
    /// Negotiate mono 16 kHz signed 16-bit capture and report what was
    /// actually installed.
    fn configure(&mut self, config: CaptureConfig) -> Result<CaptureFormat, ConfigureError>;
    fn start(&mut self) -> Result<(), DeviceFault>;
    /// At most one nonblocking read. Nothing available is a fault for which
    /// [`DeviceFault::would_block`] holds.
    fn read(&mut self, into: &mut [i16]) -> Result<usize, DeviceFault>;
    fn status(&mut self) -> Result<CaptureStatus, DeviceFault>;
    /// Stop at once and drop what is buffered. The handle itself closes when
    /// the device is dropped, whether or not this succeeded.
    fn discard(&mut self) -> Result<(), DeviceFault>;
    /// Wait in one `poll` until `frames` samples may be read (the device's
    /// `avail_min`, so a microphone that hands over whole blocks wakes once a
    /// block, as it arrives) or one of `others` is readable, for at most
    /// `timeout` (`LAMP_CAPTURE_WAIT`, #705). `None`: this device cannot be
    /// waited on, and the caller paces itself.
    fn wait(
        &mut self,
        _others: &[BorrowedFd<'_>],
        _timeout: Duration,
        _frames: usize,
    ) -> Option<Result<Woken, DeviceFault>> {
        None
    }
}

/// What ended a [`CaptureDevice::wait`]: neither is a timeout.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Woken {
    /// The device has samples, or a fault the next read will report.
    pub samples: bool,
    /// One of the other descriptors is readable or closed.
    pub others: bool,
}
