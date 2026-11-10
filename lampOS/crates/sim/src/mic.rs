//! The host's microphone as the ears' sound: 20 ms blocks of 16 kHz mono
//! S16LE on `mic1/audio`, as `lamp_msgs::Audio` carries them. The host's
//! microphone is opened by `ffmpeg` (PulseAudio on Linux, AVFoundation
//! on macOS; `LAMP_SIM_MIC` names another device), under a shell that closes
//! it when lamp-sim ends, even killed.
use lamp_bus::Bus;
use lamp_msgs::Audio;
use std::{
    io::{self, Read},
    os::unix::process::CommandExt,
    process::{Command, Stdio},
};

/// Samples in a block (20 ms at 16 kHz), and its bytes.
const BLOCK_BYTES: usize = 320 * 2;

/// Publishes the host microphone until it ends. Returns why it did.
pub fn run(bus: &Bus) -> io::Result<()> {
    let mut publisher = bus.publisher_declared(Audio::TOPICS[0], &Audio::declaration(), "ears")?;
    // Format and device reach the shell as environment values, never as text
    // of the script. `LAMP_SIM_MIC` names the device.
    let (format, default) = if cfg!(target_os = "macos") {
        ("avfoundation", ":0")
    } else {
        ("pulse", "default")
    };
    let device = std::env::var("LAMP_SIM_MIC").unwrap_or_else(|_| default.to_owned());
    let script = "ffmpeg -loglevel error -nostdin -f \"$FORMAT\" -i \"$DEVICE\" -ac 1 -ar 16000 -f s16le - & F=$!; \
         while kill -0 \"$OWNER\" 2>/dev/null && kill -0 $F 2>/dev/null; do sleep 1; done; \
         kill $F 2>/dev/null";
    let mut child = Command::new("sh")
        .args(["-c", script])
        .env("FORMAT", format)
        .env("DEVICE", &device)
        .env("OWNER", std::process::id().to_string())
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut out = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("no pipe"))?;
    let mut samples = [0u8; BLOCK_BYTES];
    let mut sequence = 0u64;
    loop {
        out.read_exact(&mut samples)?;
        let header = Audio {
            sample_rate: 16_000,
            channels: 1,
            sample_format: Audio::S16LE.to_owned(),
            sequence,
        };
        sequence += 1;
        publisher.publish_bytes(
            lamp_ipc::monotonic_us(),
            Audio::ENCODING,
            &header.block(&samples)?,
        )?;
    }
}
