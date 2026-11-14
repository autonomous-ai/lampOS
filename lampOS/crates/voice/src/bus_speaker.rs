//! The speaker worker's sound put on the bus: `--speaker-device bus:speaker1`
//! (`options::SpeakerDevice::BUS_SPEAKER1`), for a developer's computer beside `lamp-sim`, which
//! plays what the voice says (the mirror of `bus_capture`'s `--microphone-device bus:mic1`). The
//! speaker is the board's own `lamp_audio::pcm::Speaker`, with every privacy, ownership,
//! authority and underrun rule it has; only the queue below it is another, a
//! [`lamp_audio::soft_pcm::SoftPcm`], and the sink it is given publishes each block it takes on
//! `speaker1/play` ([`Audio::PLAY_TOPIC`]) as `lamp_msgs::Audio`: 24 kHz mono S16LE, one block a
//! write (up to 10 ms; a shorter one when the queue had room for only part of a write), the
//! sequence rising by one a block. Off unless asked for and in no product unit.
//!
//! The clock is the voice's: the queue drains at the rendering rate, so the speaker writes at the
//! pace a sound card would let it, and the bus never holds the voice back. A block the bus refuses
//! is counted (`Counts::refused`, said once and every hundredth) and is not waited on; its
//! sequence is spent, so the reader sees the gap. `discard` (a cut, a privacy stop) empties the
//! queue, but what was published is out: at most the queue's depth, 40 ms.
use lamp_audio::{
    pcm::Speaker,
    soft_pcm::{self, SoftPcm},
};
use lamp_bus::Bus;
use lamp_msgs::Audio;
use lamp_turn_owner::BootId;
use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

/// What this publisher is called to the bus.
const NAME: &str = "voice_speaker";
/// How long a block may wait on the bus before it is stale (as `lamp-control`'s speaker blocks).
const BLOCK_LIFETIME: Duration = Duration::from_millis(250);

/// Whether `device` (the speaker worker's device name) is the bus's.
pub fn is_bus_device(device: &str) -> bool {
    device == crate::options::SpeakerDevice::BUS_SPEAKER1
}

/// What the sink has done, for the worker's say and for tests.
#[derive(Default)]
pub struct Counts {
    pub published: AtomicU64,
    pub refused: AtomicU64,
}

/// A sink for a [`SoftPcm`] that frames each block as `lamp_msgs::Audio` and hands the payload to
/// `publish`. The sequence is spent on a block `publish` refuses.
pub fn framing_sink(
    mut publish: impl FnMut(&[u8]) -> io::Result<()> + Send + 'static,
    counts: Arc<Counts>,
) -> impl FnMut(&[i16]) + Send + 'static {
    let mut sequence = 0u64;
    move |frames: &[i16]| {
        let header = Audio {
            sample_rate: Audio::SPEAKER_SAMPLE_RATE,
            channels: 1,
            sample_format: Audio::S16LE.into(),
            sequence,
        };
        sequence += 1;
        let bytes: Vec<u8> = frames.iter().flat_map(|s| s.to_le_bytes()).collect();
        let Ok(payload) = header.block(&bytes) else {
            counts.refused.fetch_add(1, Ordering::Relaxed);
            return;
        };
        match publish(&payload) {
            Ok(()) => {
                counts.published.fetch_add(1, Ordering::Relaxed);
            }
            Err(error) => {
                let refused = counts.refused.fetch_add(1, Ordering::Relaxed) + 1;
                if refused == 1 || refused.is_multiple_of(100) {
                    eprintln!(
                        "lamp-voice speaker: the bus refused a block of the voice's sound ({refused} so far): {error}"
                    );
                }
            }
        }
    }
}

/// The queue on `bus`: the voice's sound published on `speaker1/play`.
pub fn queue_on(bus: &Bus) -> io::Result<(SoftPcm, Arc<Counts>)> {
    let mut publisher = bus.publisher_declared(
        Audio::PLAY_TOPIC,
        &Audio::speaker_declaration().lasting(BLOCK_LIFETIME),
        NAME,
    )?;
    publisher.expires_after(Some(BLOCK_LIFETIME));
    let counts = Arc::new(Counts::default());
    let sink = framing_sink(
        move |payload| {
            publisher
                .publish_bytes(lamp_ipc::monotonic_us(), Audio::ENCODING, payload)
                .map(|_| ())
        },
        counts.clone(),
    );
    Ok((SoftPcm::real_time(sink), counts))
}

/// The speaker, over the bus named by the environment (`LAMP_BUS_ROOT`, passed to the worker as to
/// the capture worker), in the clock mode the worker uses.
pub fn open(boot: BootId) -> lamp_audio::Result<Speaker> {
    let bus = Bus::from_environment()?;
    let (queue, _counts) = queue_on(&bus)?;
    Speaker::with_pcm(Box::new(queue), soft_pcm::FORMAT, boot, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_audio::pcm::PlaybackPcm;
    use std::time::Instant;

    #[test]
    fn only_bus_speaker1_is_the_bus_device() {
        assert!(is_bus_device("bus:speaker1"));
        for other in [
            "bus:mic1",
            "bus:speaker2",
            "default",
            "plug:device_speaker",
            "",
        ] {
            assert!(!is_bus_device(other), "{other}");
        }
    }

    type Seen = Arc<std::sync::Mutex<Vec<Vec<u8>>>>;

    fn payloads() -> (Arc<Counts>, Seen, impl FnMut(&[i16]) + Send) {
        let counts = Arc::new(Counts::default());
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let held = seen.clone();
        let sink = framing_sink(
            move |payload| {
                held.lock().unwrap().push(payload.to_vec());
                Ok(())
            },
            counts.clone(),
        );
        (counts, seen, sink)
    }

    #[test]
    fn each_block_is_24_khz_mono_s16le_with_the_sequence_rising_by_one() {
        let (counts, seen, mut sink) = payloads();
        sink(&[1, -2, 3]);
        sink(&[7; 240]);
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        let (first, bytes) = Audio::split(&seen[0]).unwrap();
        assert_eq!(
            (
                first.sample_rate,
                first.channels,
                first.sample_format.as_str(),
                first.sequence
            ),
            (24_000, 1, "S16LE", 0)
        );
        assert_eq!(bytes, [1, 0, 254, 255, 3, 0]);
        let (second, bytes) = Audio::split(&seen[1]).unwrap();
        assert_eq!(second.sequence, 1);
        assert_eq!(bytes.len(), 480);
        assert_eq!(counts.published.load(Ordering::Relaxed), 2);
        assert_eq!(counts.refused.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn a_block_the_bus_refuses_is_counted_and_its_sequence_is_spent() {
        let counts = Arc::new(Counts::default());
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (held, mut refuse) = (seen.clone(), true);
        let mut sink = framing_sink(
            move |payload| {
                let said = std::mem::replace(&mut refuse, false);
                if said {
                    return Err(io::Error::other("no room"));
                }
                held.lock().unwrap().push(payload.to_vec());
                Ok(())
            },
            counts.clone(),
        );
        sink(&[1; 10]);
        sink(&[2; 10]);
        assert_eq!(counts.refused.load(Ordering::Relaxed), 1);
        assert_eq!(counts.published.load(Ordering::Relaxed), 1);
        // The reader sees sequence 1 first: 0 was spent on the refused block, a gap it can count.
        let (header, _) = Audio::split(&seen.lock().unwrap()[0]).unwrap();
        assert_eq!(header.sequence, 1);
    }

    /// Through a real bus: what the speaker's queue takes is what a subscriber of `speaker1/play`
    /// reads, in order, typed as `lamp_msgs/Audio`.
    #[test]
    fn what_the_queue_takes_is_what_a_subscriber_of_the_play_topic_reads() {
        let dir = std::env::temp_dir().join(format!("lamp-bus-speaker-{}", std::process::id()));
        let bus = Bus::new(&dir).unwrap();
        let mut hearing = bus
            .subscribe_typed(Audio::PLAY_TOPIC, "sim", Audio::TYPE)
            .unwrap();
        let (queue, counts) = queue_on(&bus).unwrap();
        assert_eq!(queue.writei(&[5; 240]).unwrap(), 240);
        assert_eq!(queue.writei(&[6; 120]).unwrap(), 120);
        let mut got = Vec::new();
        let until = Instant::now() + Duration::from_secs(2);
        while got.len() < 2 && Instant::now() < until {
            if let Some(message) = hearing.recv(Duration::from_millis(50)).unwrap() {
                got.push(message);
            }
        }
        assert_eq!(got.len(), 2);
        let (first, bytes) = Audio::split(&got[0].payload).unwrap();
        assert_eq!((first.sequence, bytes.len()), (0, 480));
        let (second, bytes) = Audio::split(&got[1].payload).unwrap();
        assert_eq!((second.sequence, bytes.len()), (1, 240));
        assert_eq!(counts.published.load(Ordering::Relaxed), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
