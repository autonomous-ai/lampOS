//! The capture worker's microphone read off the bus: `--microphone-device bus:mic1`
//! (`options::MicrophoneDevice`), for a developer's computer beside `lamp-sim
//! --microphone`, which publishes the host's microphone on `mic1/audio` as
//! `lamp_msgs::Audio` (20 ms, 320 samples, 16 kHz mono S16LE). Off unless asked for and
//! in no product unit. It is a [`CaptureDevice`], so the capture loop above it is the
//! board's own and does not know the sound came off the bus. Each 20 ms block is read
//! out as whatever the loop asks for (it asks for 10 ms, 160 samples), so one block
//! is two reads, in order. Nothing is waited on (`wait` is `None`, the loop paces
//! itself), and nothing is kept: sound the loop has not read for a second is dropped,
//! oldest first.
//!
//! Pacing when the bus is idle: `wait` is `None`, so the capture loop does not sleep on this
//! device. It passes once every `audio_workers::TICK` (2 ms, `finish_tick`; with
//! `LAMP_CAPTURE_WAIT=on` a device with no `wait` is ticked the same way), and each pass calls
//! `read` and `status`, which take in the bus with `recv(Duration::ZERO)`: a non-blocking
//! receive that finds nothing is an idle tick, not an error (`take_in` returns, `read` then says
//! `EAGAIN`, not ready, which the loop treats as no sound yet). An idle bus is about 500 such
//! passes a second, never a busy loop. What that costs a laptop's CPU is for the lamp-sim run that uses it to measure.
use lamp_audio::{
    capture::{CaptureConfig, CaptureFormat},
    device::{CaptureDevice, CaptureStatus, ConfigureError, DeviceFault, DeviceState, errno},
};
use lamp_bus::{Bus, Subscriber};
use lamp_msgs::Audio;
use std::{collections::VecDeque, time::Duration};

/// The topic read.
const TOPIC: &str = "mic1/audio";
/// What this reader is called to the bus.
const NAME: &str = "voice_capture";
/// The most sound held for a slow reader: a second at 16 kHz.
const HELD_MAX: usize = 16_000;
const OPEN_CALL: &str = "bus_subscribe";

pub struct BusCapture {
    sound: Subscriber,
    held: VecDeque<i16>,
    config: CaptureConfig,
    /// Whether a malformed block has been said once already.
    said_malformed: bool,
}

/// Whether `device` (a capture worker's device name) is the bus's.
pub fn is_bus_device(device: &str) -> bool {
    device == crate::options::MicrophoneDevice::BUS_MIC1
}

impl BusCapture {
    /// Reading `mic1/audio` of `bus`.
    pub fn on(bus: &Bus) -> std::io::Result<Self> {
        Ok(Self {
            sound: bus.subscribe_typed(TOPIC, NAME, Audio::TYPE)?,
            held: VecDeque::new(),
            config: CaptureConfig::default(),
            said_malformed: false,
        })
    }

    /// Takes in every block the bus has now, none waited for.
    fn take_in(&mut self) -> Result<(), DeviceFault> {
        loop {
            let message = self.sound.recv(Duration::ZERO).map_err(|error| {
                DeviceFault::new("bus_recv", error.raw_os_error().unwrap_or(errno::IO))
            })?;
            let Some(message) = message else {
                return Ok(());
            };
            // A block that is not mono 16 kHz S16LE is not sound this loop can use.
            let usable = Audio::split(&message.payload)
                .ok()
                .filter(|(header, bytes)| {
                    header.sample_rate == 16_000
                        && header.channels == 1
                        && header.sample_format == Audio::S16LE
                        && bytes.len() % 2 == 0
                });
            let Some((_, bytes)) = usable else {
                if !self.said_malformed {
                    self.said_malformed = true;
                    eprintln!(
                        "[voice] bus mic1: dropped a block that is not mono 16 kHz S16LE (further ones not logged)"
                    );
                }
                continue;
            };
            self.held.extend(
                bytes
                    .chunks_exact(2)
                    .map(|pair| i16::from_le_bytes([pair[0], pair[1]])),
            );
            if self.held.len() > HELD_MAX {
                let extra = self.held.len() - HELD_MAX;
                self.held.drain(..extra);
            }
        }
    }
}

impl CaptureDevice for BusCapture {
    fn open(name: &str) -> Result<Self, DeviceFault> {
        if !is_bus_device(name) {
            return Err(DeviceFault::new(OPEN_CALL, errno::NOENT));
        }
        Bus::from_environment()
            .and_then(|bus| Self::on(&bus))
            .map_err(|error| DeviceFault::new(OPEN_CALL, error.raw_os_error().unwrap_or(errno::IO)))
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
        Ok(())
    }

    fn read(&mut self, into: &mut [i16]) -> Result<usize, DeviceFault> {
        self.take_in()?;
        if self.held.is_empty() {
            return Err(DeviceFault::new("read", errno::AGAIN));
        }
        let frames = into.len().min(self.held.len());
        for (target, sample) in into.iter_mut().zip(self.held.drain(..frames)) {
            *target = sample;
        }
        Ok(frames)
    }

    fn status(&mut self) -> Result<CaptureStatus, DeviceFault> {
        self.take_in()?;
        Ok(CaptureStatus {
            state: DeviceState::Running,
            monotonic_us: None,
            available: self.held.len() as i64,
            delayed: self.held.len() as i64,
        })
    }

    fn discard(&mut self) -> Result<(), DeviceFault> {
        self.held.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(first: i16) -> Vec<u8> {
        let samples: Vec<u8> = (0..320i16)
            .flat_map(|k| (first + k).to_le_bytes())
            .collect();
        let header = Audio {
            sample_rate: 16_000,
            channels: 1,
            sample_format: Audio::S16LE.to_owned(),
            sequence: 0,
        };
        header.block(&samples).unwrap()
    }

    fn read_when_ready(device: &mut BusCapture, into: &mut [i16]) -> Result<usize, DeviceFault> {
        for _ in 0..100 {
            match device.read(into) {
                Err(fault) if fault.would_block() => std::thread::sleep(Duration::from_millis(10)),
                other => return other,
            }
        }
        device.read(into)
    }

    #[test]
    fn a_20_ms_block_is_read_as_two_10_ms_blocks_in_order_then_nothing_is_ready() {
        let dir = std::env::temp_dir().join(format!("lamp-bus-capture-{}", std::process::id()));
        let bus = Bus::new(&dir).unwrap();
        let mut publisher = bus
            .publisher_declared(TOPIC, &Audio::declaration(), "ears")
            .unwrap();
        let mut device = BusCapture::on(&bus).unwrap();
        let mut into = [0i16; 160];
        assert!(
            device.read(&mut into).unwrap_err().would_block(),
            "nothing yet"
        );
        publisher
            .publish_bytes(lamp_ipc::monotonic_us(), Audio::ENCODING, &block(0))
            .unwrap();
        assert_eq!(read_when_ready(&mut device, &mut into).unwrap(), 160);
        assert_eq!(into[..3], [0, 1, 2]);
        assert_eq!(into[159], 159);
        assert_eq!(device.read(&mut into).unwrap(), 160);
        assert_eq!((into[0], into[159]), (160, 319));
        assert!(device.read(&mut into).unwrap_err().would_block(), "a gap");
        // Two blocks at once come out in order; a discard drops them.
        publisher
            .publish_bytes(lamp_ipc::monotonic_us(), Audio::ENCODING, &block(1000))
            .unwrap();
        publisher
            .publish_bytes(lamp_ipc::monotonic_us(), Audio::ENCODING, &block(2000))
            .unwrap();
        assert_eq!(read_when_ready(&mut device, &mut into).unwrap(), 160);
        assert_eq!(into[0], 1000);
        std::thread::sleep(Duration::from_millis(50));
        assert!(device.status().unwrap().available >= 160);
        device.discard().unwrap();
        assert!(device.read(&mut into).unwrap_err().would_block());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Every text file under `root`, recursively, with its path.
    fn text_files(root: &std::path::Path, found: &mut Vec<(std::path::PathBuf, String)>) {
        let Ok(entries) = std::fs::read_dir(root) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                text_files(&path, found);
            } else if let Ok(text) = std::fs::read_to_string(&path) {
                found.push((path, text));
            }
        }
    }

    /// The developer's `--microphone-device` is for a laptop beside lamp-sim: no unit, deploy
    /// file or updater source of the product may name it, or a lamp would read its microphone
    /// off the bus and hear nothing.
    #[test]
    fn the_microphone_device_is_in_no_product_unit_deploy_file_or_update_source() {
        let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        for root in ["../../deploy", "../update/src", "../../scripts"] {
            text_files(&here.join(root), &mut files);
        }
        // An empty walk cannot pass: it found the product's own voice unit.
        assert!(
            files
                .iter()
                .any(|(path, _)| path.ends_with("units/lamp-voice.service")),
            "the walk did not find deploy/units/lamp-voice.service ({} files)",
            files.len()
        );
        for (path, text) in &files {
            assert!(
                !text.contains("microphone-device"),
                "the developer's --microphone-device is in no product unit: {}",
                path.display()
            );
        }
    }

    #[test]
    fn only_the_bus_spelling_is_a_bus_device_and_a_board_name_is_refused() {
        assert!(is_bus_device("bus:mic1"));
        assert!(!is_bus_device("plug:device_micro1"));
        assert!(!is_bus_device("bus:mic2"));
        assert!(BusCapture::open("plug:device_micro2").is_err());
    }

    #[test]
    fn a_block_that_is_not_mono_16_khz_is_dropped_and_said_once() {
        let dir = std::env::temp_dir().join(format!("lamp-bus-capture-bad-{}", std::process::id()));
        let bus = Bus::new(&dir).unwrap();
        let mut publisher = bus
            .publisher_declared(TOPIC, &Audio::declaration(), "ears")
            .unwrap();
        let mut device = BusCapture::on(&bus).unwrap();
        let stereo = Audio {
            sample_rate: 16_000,
            channels: 2,
            sample_format: Audio::S16LE.to_owned(),
            sequence: 0,
        }
        .block(&[0u8; 640])
        .unwrap();
        for _ in 0..3 {
            publisher
                .publish_bytes(lamp_ipc::monotonic_us(), Audio::ENCODING, &stereo)
                .unwrap();
        }
        publisher
            .publish_bytes(lamp_ipc::monotonic_us(), Audio::ENCODING, &block(7))
            .unwrap();
        let mut into = [0i16; 160];
        assert_eq!(read_when_ready(&mut device, &mut into).unwrap(), 160);
        assert_eq!(into[0], 7, "only the good block was kept");
        assert!(device.said_malformed, "said");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
