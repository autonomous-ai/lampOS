//! The ALSA calls behind [`crate::device`]. Each method is the call the
//! boundary made directly before the trait existed, and nothing more.
use crate::{
    CAPTURE_RATE, CAPTURE_SAMPLES,
    capture::{CaptureConfig, CaptureFormat, valid_format},
    device::{
        CaptureDevice, CaptureStatus, ConfigureError, DeviceFault, DeviceState, Woken, timestamp_us,
    },
};
use alsa::{
    Direction, ValueOr,
    pcm::{Access, Format, HwParams, PCM, State, TstampType},
};
use std::{
    os::fd::{AsRawFd, BorrowedFd},
    time::Duration,
};

fn fault(error: alsa::Error) -> DeviceFault {
    DeviceFault::new(error.func(), error.errno())
}

fn state(state: State) -> DeviceState {
    match state {
        State::Setup => DeviceState::Setup,
        State::Prepared => DeviceState::Prepared,
        State::Running => DeviceState::Running,
        State::XRun => DeviceState::XRun,
        State::Draining => DeviceState::Draining,
        State::Suspended => DeviceState::Suspended,
        State::Disconnected => DeviceState::Disconnected,
        _ => DeviceState::Other,
    }
}

/// The PCM, and the `avail_min` installed on it.
pub struct AlsaCapture(PCM, i64);

impl CaptureDevice for AlsaCapture {
    fn open(name: &str) -> Result<Self, DeviceFault> {
        PCM::new(name, Direction::Capture, true)
            .map(|pcm| Self(pcm, 1))
            .map_err(fault)
    }

    fn configure(&mut self, config: CaptureConfig) -> Result<CaptureFormat, ConfigureError> {
        let pcm = &self.0;
        let failed = |error: alsa::Error| ConfigureError::Fault(fault(error));
        let hardware = HwParams::any(pcm).map_err(failed)?;
        hardware.set_access(Access::RWInterleaved).map_err(failed)?;
        hardware.set_format(Format::s16()).map_err(failed)?;
        hardware.set_channels(1).map_err(failed)?;
        hardware
            .set_rate(CAPTURE_RATE, ValueOr::Nearest)
            .map_err(failed)?;
        hardware
            .set_period_size_near(CAPTURE_SAMPLES as i64, ValueOr::Nearest)
            .map_err(failed)?;
        let requested_buffer = config.requested_frames();
        hardware
            .set_buffer_size_near(requested_buffer)
            .map_err(failed)?;
        pcm.hw_params(&hardware).map_err(failed)?;
        let current = pcm.hw_params_current().map_err(failed)?;
        let mut format = CaptureFormat {
            rate: current.get_rate().map_err(failed)?,
            channels: current.get_channels().map_err(failed)?,
            period_frames: current.get_period_size().map_err(failed)?,
            buffer_frames: current.get_buffer_size().map_err(failed)?,
            monotonic_status_timestamps: false,
        };
        if current.get_format().map_err(failed)? != Format::s16()
            || current.get_access().map_err(failed)? != Access::RWInterleaved
            || !valid_format(format, requested_buffer)
        {
            return Err(ConfigureError::Unsupported(format));
        }
        let software = pcm.sw_params_current().map_err(failed)?;
        software.set_avail_min(1).map_err(failed)?;
        software.set_start_threshold(1).map_err(failed)?;
        // Timestamp support is optional; inability is reported in negotiated format.
        let timestamps = software.set_tstamp_type(TstampType::Monotonic).is_ok()
            && software.set_tstamp_mode(true).is_ok();
        if !timestamps {
            software.set_tstamp_mode(false).map_err(failed)?;
        }
        pcm.sw_params(&software).map_err(failed)?;
        self.1 = 1;
        let installed = pcm.sw_params_current().map_err(failed)?;
        format.monotonic_status_timestamps = installed.get_tstamp_mode().map_err(failed)?
            && installed.get_tstamp_type().map_err(failed)? == TstampType::Monotonic;
        Ok(format)
    }

    fn start(&mut self) -> Result<(), DeviceFault> {
        self.0.start().map_err(fault)
    }

    fn read(&mut self, into: &mut [i16]) -> Result<usize, DeviceFault> {
        self.0
            .io_i16()
            .and_then(|reader| reader.readi(into))
            .map_err(fault)
    }

    fn status(&mut self) -> Result<CaptureStatus, DeviceFault> {
        let status = self.0.status().map_err(fault)?;
        let stamp = status.get_htstamp();
        Ok(CaptureStatus {
            state: state(status.get_state()),
            monotonic_us: timestamp_us(stamp.tv_sec, stamp.tv_nsec),
            available: status.get_avail(),
            delayed: status.get_delay(),
        })
    }

    fn discard(&mut self) -> Result<(), DeviceFault> {
        self.0.drop().map_err(fault)
    }

    /// `avail_min` set to `frames` (only when it changes, so once for a
    /// microphone read a whole block at a time), then
    /// `snd_pcm_poll_descriptors` and the others in one `poll`; the PCM's
    /// own `revents` are read through `snd_pcm_poll_descriptors_revents`. A
    /// PCM descriptor that woke without samples (a plugin's) ends the wait
    /// with neither, and the caller ticks before it waits again, so a
    /// descriptor left readable is never spun on.
    fn wait(
        &mut self,
        others: &[BorrowedFd<'_>],
        timeout: Duration,
        frames: usize,
    ) -> Option<Result<Woken, DeviceFault>> {
        use alsa::poll::{Descriptors, Flags, pollfd};
        let frames = i64::try_from(frames.max(1)).unwrap_or(i64::MAX);
        if frames != self.1 {
            let installed = self.0.sw_params_current().and_then(|software| {
                software.set_avail_min(frames)?;
                self.0.sw_params(&software)
            });
            if let Err(error) = installed {
                return Some(Err(fault(error)));
            }
            self.1 = frames;
        }
        let mut fds = match self.0.get() {
            Ok(pcm) if !pcm.is_empty() => pcm,
            Ok(_) => return None,
            Err(error) => return Some(Err(fault(error))),
        };
        let count = fds.len();
        fds.extend(others.iter().map(|other| pollfd {
            fd: other.as_raw_fd(),
            events: Flags::IN.bits(),
            revents: 0,
        }));
        // Rounded up: a wait never ends before its bound.
        let timeout_ms = i32::try_from(timeout.as_micros().div_ceil(1_000)).unwrap_or(i32::MAX);
        if let Err(error) = alsa::poll::poll(&mut fds, timeout_ms) {
            return Some(Err(fault(error)));
        }
        let samples = match self.0.revents(&fds[..count]) {
            Ok(flags) => flags.intersects(Flags::IN | Flags::ERR | Flags::HUP),
            Err(error) => return Some(Err(fault(error))),
        };
        let others = fds[count..].iter().any(|fd| fd.revents != 0);
        Some(Ok(Woken { samples, others }))
    }
}
