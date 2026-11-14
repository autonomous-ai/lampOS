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
    process::Stdio,
};

/// Samples in a block (20 ms at 16 kHz), and its bytes.
const BLOCK_BYTES: usize = 320 * 2;

/// Publishes the host microphone until it ends. Returns why it did.
pub fn run(bus: &Bus) -> io::Result<()> {
    let mut publisher = bus.publisher_declared(Audio::TOPICS[0], &Audio::declaration(), "ears")?;
    // Format and device reach the shell as environment values, never as text
    // of the script. `LAMP_SIM_MIC` names the device.
    let (host, default) = if cfg!(target_os = "macos") {
        ("avfoundation", ":0")
    } else {
        ("pulse", "default")
    };
    let given = std::env::var("LAMP_SIM_MIC").unwrap_or_else(|_| default.to_owned());
    let (format, device, pace) = input_of(host, &given);
    // `$PACE` is this file's own word (`-re` or nothing), never the developer's text.
    let script = "ffmpeg -loglevel error -nostdin $PACE -f \"$FORMAT\" -i \"$DEVICE\" -ac 1 -ar 16000 -f s16le - & F=$!; \
         while kill -0 \"$OWNER\" 2>/dev/null && kill -0 $F 2>/dev/null; do sleep 1; done; \
         kill $F 2>/dev/null";
    let mut child = crate::keys::command("sh")
        .args(["-c", script])
        .env("FORMAT", format)
        .env("DEVICE", &device)
        .env("PACE", pace)
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

/// What ffmpeg opens for `LAMP_SIM_MIC`: the host's microphone (`host` is its
/// input format), or, when the value starts with `lavfi:`, a sound ffmpeg makes
/// itself (`lavfi:sine=frequency=440`), read at its own pace (`-re`: a made
/// source would otherwise come faster than real time and flood the bus), with
/// no host device opened: a machine with no microphone, or a test that must
/// hear the same sound every time. As with `LAMP_SIM_CAMERA`, a lavfi source
/// can read local files (`amovie=...`), so `LAMP_SIM_MIC` is the developer's own
/// setting, never taken from a request.
fn input_of(host: &'static str, given: &str) -> (&'static str, String, &'static str) {
    match given.strip_prefix("lavfi:") {
        Some(source) => ("lavfi", source.to_owned(), "-re"),
        None => (host, given.to_owned(), ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lavfi_value_is_a_made_sound_at_its_own_pace_and_anything_else_the_hosts_microphone() {
        assert_eq!(
            input_of("avfoundation", "lavfi:sine=frequency=440"),
            ("lavfi", "sine=frequency=440".to_owned(), "-re")
        );
        assert_eq!(
            input_of("avfoundation", ":0"),
            ("avfoundation", ":0".to_owned(), "")
        );
        assert_eq!(
            input_of("pulse", "default"),
            ("pulse", "default".to_owned(), "")
        );
        // Only a leading `lavfi:` makes it one.
        assert_eq!(
            input_of("pulse", "x lavfi:sine"),
            ("pulse", "x lavfi:sine".to_owned(), "")
        );
    }
}
