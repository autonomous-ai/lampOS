//! The sleep capture worker (migrate-P2, `LAMP_SLEEP_HEARS_NAME`; the Queue
//! comment <https://github.com/autonomous-ai/lampOS/issues/3997#issuecomment-6526292114>).
//!
//! While a hand's sleep waits with the privacy slide open, this worker reads
//! the second microphone and hands its 20 ms blocks to the voice, which
//! publishes them for the name listener alone (`mic2/sleep_audio`,
//! [`crate::sleep_capture`]). It is the narrowest worker the voice has: one
//! microphone, no speaker, no echo reference, no speech detector, no speech
//! to text and no `MicOut`. It takes no [`Control::Authority`]: the wait's
//! snapshot stays `Denied` for microphone and camera, so "Denied" still means
//! the main capture is silent. Its only authority is the wait's separate
//! name-only grant ([`lamp_turn_owner::NameGrant`]), held by a
//! [`NameGrantGuard`] bound to the voice that started it.
//!
//! The microphone opens only under a standing grant and is read only while
//! it stands, checked again before each block leaves. A revocation (the
//! slide closed or its observation went stale, or the sleep is over) or a
//! lease that lapses closes the microphone at once and ends the worker
//! cleanly: it never reopens under the same process, so a new grant needs a
//! new worker, started by the wait after verified cleanup of this one.
use crate::{
    transport::WorkerChannels,
    wire::{Control, SleepBlock, WorkerEvent},
};
use lamp_audio::{
    capture::CaptureConfig,
    device::{CaptureDevice, ConfigureError},
};
use lamp_ipc::monotonic_us;
use lamp_turn_owner::{BootId, MonoTime, NameGrantGuard};
use std::{
    io,
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// One block: 20 ms at 16 kHz, as `mic2/audio`'s (`lamp_msgs::Audio`).
pub const BLOCK_SAMPLES: usize = 320;
/// How often the loop looks at its grant, its controls and the microphone.
const TICK: Duration = Duration::from_millis(5);
/// How long a new worker waits for its first grant before it ends: the wait
/// sends one at once after the worker's Ready.
pub const FIRST_GRANT_WAIT: Duration = Duration::from_secs(1);
/// At most this many controls are read in one look.
const MAX_CONTROL: usize = 16;

/// The worker on the board's microphone, by its ALSA name.
#[cfg(target_os = "linux")]
pub fn run_alsa(channels: WorkerChannels, controller: BootId, device: &str) -> Result<()> {
    run::<lamp_audio::device::AlsaCapture>(channels, controller, device)
}

/// The worker over any capture device: ALSA on a lamp, a scripted one in a
/// test, with the same loop.
pub fn run<D: CaptureDevice>(
    mut channels: WorkerChannels,
    controller: BootId,
    device: &str,
) -> Result<()> {
    channels.control.send(WorkerEvent::Ready)?;
    let mut guard = NameGrantGuard::new(controller);
    let mut microphone: Option<D> = None;
    let result = serve(&mut channels, &mut guard, &mut microphone, device);
    // Closed before anything is said about it: the device is let go first.
    if let Some(mut open) = microphone.take() {
        let _ = open.discard();
    }
    let _ = channels.control.send(WorkerEvent::CaptureStopped);
    result
}

fn serve<D: CaptureDevice>(
    channels: &mut WorkerChannels,
    guard: &mut NameGrantGuard,
    microphone: &mut Option<D>,
    device: &str,
) -> Result<()> {
    let started = Instant::now();
    let mut pending: Vec<i16> = Vec::with_capacity(BLOCK_SAMPLES * 4);
    let mut read = vec![0i16; BLOCK_SAMPLES * 4];
    let mut sequence = 0u64;
    loop {
        for _ in 0..MAX_CONTROL {
            match channels.control.receive::<Control>()? {
                Some(Control::NameGrant { grant }) => guard
                    .install(MonoTime::from_micros(monotonic_us()), grant)
                    .map_err(|error| io::Error::other(format!("name grant refused: {error}")))?,
                Some(Control::Stop) => return Ok(()),
                // Never a snapshot, a reference or a capture start: this
                // worker holds no other authority and takes none.
                Some(_) => {
                    return Err(io::Error::other(
                        "the sleep capture takes only its name grant and Stop",
                    )
                    .into());
                }
                None => break,
            }
        }
        if !guard.allows(MonoTime::from_micros(monotonic_us())) {
            // Revoked or lapsed: the microphone closes as this returns, and
            // this worker never opens it again.
            if microphone.is_some() || guard.ever_granted() {
                return Ok(());
            }
            if started.elapsed() >= FIRST_GRANT_WAIT {
                return Err(io::Error::other("no name grant came to the sleep capture").into());
            }
            std::thread::sleep(TICK);
            continue;
        }
        if microphone.is_none() {
            *microphone = Some(open::<D>(device)?);
        }
        let Some(open) = microphone.as_mut() else {
            continue;
        };
        for _ in 0..4 {
            match open.read(&mut read) {
                Ok(0) => break,
                Ok(frames) => pending.extend_from_slice(&read[..frames]),
                Err(fault) if fault.would_block() => break,
                Err(fault) => return Err(io::Error::other(fault.to_string()).into()),
            }
        }
        let read_at = monotonic_us();
        while pending.len() >= BLOCK_SAMPLES {
            // Checked again for each block, at the moment it leaves.
            if !guard.allows(MonoTime::from_micros(monotonic_us())) {
                pending.clear();
                break;
            }
            let block = SleepBlock {
                sequence,
                read_completed_at_us: read_at,
                samples: pending.drain(..BLOCK_SAMPLES).collect(),
            };
            match channels.data.send(&block) {
                Ok(()) => {}
                // A voice held up loses a block, as the bus would: the name
                // listener starts afresh after a gap. Nothing waits for room.
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(error.into()),
            }
            sequence = sequence
                .checked_add(1)
                .ok_or_else(|| io::Error::other("sleep capture block count exhausted"))?;
        }
        std::thread::sleep(TICK);
    }
}

/// The microphone opened, configured to one channel at 16 kHz, and started.
fn open<D: CaptureDevice>(device: &str) -> Result<D> {
    let mut microphone = D::open(device).map_err(|fault| io::Error::other(fault.to_string()))?;
    let format = microphone
        .configure(CaptureConfig::default())
        .map_err(|error| match error {
            ConfigureError::Fault(fault) => io::Error::other(fault.to_string()),
            ConfigureError::Unsupported(format) => {
                io::Error::other(format!("sleep capture format refused: {format:?}"))
            }
        })?;
    if format.rate != 16_000 || format.channels != 1 {
        let _ = microphone.discard();
        return Err(io::Error::other(format!("sleep capture format refused: {format:?}")).into());
    }
    microphone
        .start()
        .map_err(|fault| io::Error::other(fault.to_string()))?;
    Ok(microphone)
}
