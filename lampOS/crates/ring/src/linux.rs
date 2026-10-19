//! Linux-only SPI3.0 transport. Configuration readback is reported, not confused
//! with a measured SPI waveform or optical feedback from the LEDs.

use crate::{EncodedFrame, FrameSink, GuardedRing, SPI_SPEED_HZ, waking::Lock};
use lamp_turn_owner::BootId;
use spidev::{SpiModeFlags, Spidev, SpidevOptions};
use std::io;

pub const DEVICE_PATH: &str = "/dev/spidev3.0";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpiConfiguration {
    pub bits_per_word: u8,
    pub max_speed_hz: u32,
    pub lsb_first: bool,
    pub mode_bits: u32,
}

/// Construct only through `open`, `open_privacy`, `open_asleep`, `open_setup` or `open_waking`; the device handle cannot be borrowed for an
/// unguarded public write. The advisory lock excludes cooperating lampOS owners;
/// legacy services and other writers must also be stopped by the supervisor.
pub struct SpiSink {
    device: Spidev,
}

impl FrameSink for SpiSink {
    fn write_frame(&mut self, frame: &EncodedFrame) -> io::Result<()> {
        // One syscall, never write_all: continuing a short write could split the
        // timing-sensitive WS2812 frame. Report failure and let the guard blank.
        crate::write_once(&mut self.device, frame)
    }
}

/// Open the fixed Lamp ring device, lock/configure it, validate readback and send
/// a startup black frame. No conversational light is authorized by opening.
///
/// This returns accepted driver settings, not a measurement of actual bus speed.
/// Linux spidev has no per-write timeout; run this writer in its own supervised
/// process and handle kernel/worker stalls without blocking voice capture.
///
/// It first asks the waking face for the ring ([`crate::waking::let_go`],
/// #1681), which fades out, and waits at most 2 s for it: a ring still
/// locked by it would stay off all session. With no waking face it goes on
/// at once.
pub fn open(controller_boot: BootId) -> io::Result<(GuardedRing<SpiSink>, SpiConfiguration)> {
    crate::waking::let_go("lamp-voice ring", crate::waking::Ask::Fade);
    open_guarded(controller_boot)
}

/// [`open`] for the physical-mute indication: the waking face is asked to
/// let go at once, with no fade, and waited for at most 100 ms; a ring it
/// still holds then is taken without the lock ([`crate::waking::take`]), so
/// the privacy switch's light never depends on it (#1681).
pub fn open_privacy(
    controller_boot: BootId,
) -> io::Result<(GuardedRing<SpiSink>, SpiConfiguration)> {
    let (device, configuration) = crate::waking::take(open_device)?;
    guarded(device, configuration, controller_boot)
}

fn open_guarded(controller_boot: BootId) -> io::Result<(GuardedRing<SpiSink>, SpiConfiguration)> {
    let (device, configuration) = open_device(Lock::Locked)?;
    guarded(device, configuration, controller_boot)
}

fn guarded(
    device: Spidev,
    configuration: SpiConfiguration,
    controller_boot: BootId,
) -> io::Result<(GuardedRing<SpiSink>, SpiConfiguration)> {
    let ring = GuardedRing::new(SpiSink { device }, controller_boot).map_err(io::Error::other)?;
    Ok((ring, configuration))
}

/// Open the same device, locked and configured as [`open`] does, for the
/// asleep face alone ([`crate::asleep`]). It sends black first. While the
/// asleep writer is open the ring worker cannot lock the device, so the
/// voice closes this before it starts one. Like [`open`], it first asks the
/// waking face for the ring.
pub fn open_asleep() -> io::Result<crate::asleep::AsleepRing<SpiSink>> {
    crate::waking::let_go("lamp-voice asleep face", crate::waking::Ask::Fade);
    let (device, _) = open_device(Lock::Locked)?;
    crate::asleep::AsleepRing::new(SpiSink { device })
}

/// Open the same device, locked and configured as [`open`] does, for the
/// setup light alone ([`crate::setup`]). It sends black first. While it is
/// open the voice's ring worker cannot lock the device, so `lamp-setup`
/// closes it before the lamp starts.
pub fn open_setup() -> io::Result<crate::setup::SetupRing<SpiSink>> {
    let (device, _) = open_device(Lock::Locked)?;
    crate::setup::SetupRing::new(SpiSink { device })
}

/// Open the same device, locked and configured as [`open`] does, for the
/// waking face alone ([`crate::waking`], #1681). It sends black first, and
/// asks nobody for the ring: it is the one that is asked.
pub fn open_waking() -> io::Result<crate::waking::WakingRing<SpiSink>> {
    let (device, _) = open_device(Lock::Locked)?;
    crate::waking::WakingRing::new(SpiSink { device })
}

/// Only the physical-mute indication's open asks for [`Lock::Unlocked`].
fn open_device(lock: Lock) -> io::Result<(Spidev, SpiConfiguration)> {
    let mut device = Spidev::open(DEVICE_PATH)?;
    if lock == Lock::Locked {
        device.inner().try_lock().map_err(|error| {
            let error = match error {
                std::fs::TryLockError::WouldBlock => io::Error::from(io::ErrorKind::WouldBlock),
                std::fs::TryLockError::Error(error) => error,
            };
            io::Error::new(error.kind(), format!("ring device lock failed: {error}"))
        })?;
    }
    let requested = SpidevOptions::new()
        .bits_per_word(8)
        .max_speed_hz(SPI_SPEED_HZ)
        .lsb_first(false)
        .mode(SpiModeFlags::SPI_MODE_0)
        .build();
    device.configure(&requested)?;
    let accepted = device.query_configuration()?;
    let configuration = validate_configuration(accepted)?;
    Ok((device, configuration))
}

fn validate_configuration(options: SpidevOptions) -> io::Result<SpiConfiguration> {
    let expected = SpidevOptions::new()
        .bits_per_word(8)
        .max_speed_hz(SPI_SPEED_HZ)
        .lsb_first(false)
        .mode(SpiModeFlags::SPI_MODE_0)
        .build();
    if options.bits_per_word != expected.bits_per_word
        || options.max_speed_hz != expected.max_speed_hz
        || options.lsb_first != expected.lsb_first
        || options.spi_mode != expected.spi_mode
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "SPI settings readback differs from required Lamp ring configuration: {options:?}"
            ),
        ));
    }
    Ok(SpiConfiguration {
        bits_per_word: 8,
        max_speed_hz: SPI_SPEED_HZ,
        lsb_first: false,
        mode_bits: SpiModeFlags::SPI_MODE_0.bits(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_requires_exact_waveform_settings() {
        let valid = SpidevOptions::new()
            .bits_per_word(8)
            .max_speed_hz(SPI_SPEED_HZ)
            .lsb_first(false)
            .mode(SpiModeFlags::SPI_MODE_0)
            .build();
        assert_eq!(
            validate_configuration(valid).unwrap().max_speed_hz,
            SPI_SPEED_HZ
        );
        let mut variants = [valid; 5];
        variants[0].bits_per_word = Some(16);
        variants[1].max_speed_hz = Some(6_000_000);
        variants[2].lsb_first = Some(true);
        variants[3].spi_mode = Some(SpiModeFlags::SPI_MODE_1);
        variants[4].max_speed_hz = None;
        for invalid in variants {
            assert_eq!(
                validate_configuration(invalid).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        }
    }
}
