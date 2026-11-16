//! A microphone that hears nothing: `--microphone-device silent`, taken by `directed-fixture`
//! alone (`fixture_provider::FixtureOptions::parse`), never by `serve` or `directed`.
//!
//! Where no microphone may open (a Mac, until its privacy rule is written and read twice), a
//! fixture run still has to start its workers and reach `listening_ready`. This device gives
//! the capture loop zeros at the real-time pace of 16 kHz mono and reads nothing: no card, no
//! bus, no host microphone, so there is nothing for a person to be heard through.
use lamp_audio::{
    capture::{CaptureConfig, CaptureFormat},
    device::{CaptureDevice, CaptureStatus, ConfigureError, DeviceFault, DeviceState, errno},
};
use std::time::Instant;

/// The only name this device is opened by.
pub const NAME: &str = crate::options::MicrophoneDevice::SILENT;
const RATE: u128 = 16_000;
/// The most silence held for a loop that is slow to read: a second.
const HELD_MAX: i64 = 16_000;

pub struct SilentCapture {
    began: Instant,
    /// Frames already handed out or discarded.
    taken: i64,
    config: CaptureConfig,
}

/// Whether `device` (a capture worker's device name) is this one.
pub fn is_silent_device(device: &str) -> bool {
    device == NAME
}

impl SilentCapture {
    fn owed(&mut self) -> i64 {
        let made = (self.began.elapsed().as_micros() * RATE / 1_000_000) as i64;
        // Sound the loop has not read for a second is dropped, as the bus's is.
        if made - self.taken > HELD_MAX {
            self.taken = made - HELD_MAX;
        }
        made - self.taken
    }
}

impl CaptureDevice for SilentCapture {
    fn open(name: &str) -> Result<Self, DeviceFault> {
        if !is_silent_device(name) {
            return Err(DeviceFault::new(
                lamp_audio::device::OPEN_CALL,
                errno::NOENT,
            ));
        }
        Ok(Self {
            began: Instant::now(),
            taken: 0,
            config: CaptureConfig::default(),
        })
    }

    fn configure(&mut self, config: CaptureConfig) -> Result<CaptureFormat, ConfigureError> {
        self.config = config;
        Ok(CaptureFormat {
            rate: 16_000,
            channels: 1,
            period_frames: 160,
            buffer_frames: config.requested_frames(),
            monotonic_status_timestamps: false,
        })
    }

    fn start(&mut self) -> Result<(), DeviceFault> {
        self.began = Instant::now();
        self.taken = 0;
        Ok(())
    }

    fn read(&mut self, into: &mut [i16]) -> Result<usize, DeviceFault> {
        let owed = self.owed();
        if owed <= 0 {
            return Err(DeviceFault::new("read", errno::AGAIN));
        }
        let frames = into.len().min(owed as usize);
        into[..frames].fill(0);
        self.taken += frames as i64;
        Ok(frames)
    }

    fn status(&mut self) -> Result<CaptureStatus, DeviceFault> {
        let owed = self.owed();
        Ok(CaptureStatus {
            state: DeviceState::Running,
            monotonic_us: None,
            available: owed,
            delayed: owed,
        })
    }

    fn discard(&mut self) -> Result<(), DeviceFault> {
        let owed = self.owed();
        self.taken += owed;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn open() -> SilentCapture {
        let mut device = SilentCapture::open(NAME).unwrap();
        device.start().unwrap();
        device
    }

    #[test]
    fn it_opens_by_its_own_name_and_no_other() {
        for name in [
            "bus:mic1",
            "plug:device_mic",
            "default",
            "hw:0,0",
            "",
            "Silent",
        ] {
            assert!(SilentCapture::open(name).is_err(), "{name:?}");
        }
        assert!(SilentCapture::open("silent").is_ok());
    }

    #[test]
    fn it_gives_only_zeros_and_only_as_fast_as_real_time() {
        let mut device = open();
        let mut into = [7i16; 160];
        // At once, almost nothing is owed.
        assert!(device.read(&mut into).is_err());
        std::thread::sleep(Duration::from_millis(30));
        let frames = device.read(&mut into).unwrap();
        assert!(frames > 0 && frames <= 160);
        assert!(into[..frames].iter().all(|sample| *sample == 0));
        // Thirty milliseconds is 480 frames at most, whatever the loop asks for.
        let mut total = frames;
        while let Ok(more) = device.read(&mut into) {
            total += more;
        }
        assert!(total <= 16 * 60, "{total} frames in about 30 ms");
    }

    #[test]
    fn what_the_loop_does_not_read_for_a_second_is_dropped() {
        let mut device = open();
        std::thread::sleep(Duration::from_millis(1100));
        let status = device.status().unwrap();
        assert!(status.available <= HELD_MAX, "{}", status.available);
        device.discard().unwrap();
        assert!(device.status().unwrap().available < 800);
    }
}
