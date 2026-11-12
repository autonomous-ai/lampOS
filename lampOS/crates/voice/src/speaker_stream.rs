//! A paired computer's sound for the speaker, taken in off the bus while a
//! stream of its is open (`speaker.stream`, docs/control.md "The speaker").
//!
//! lamp-control opens a stream on the say socket (`{"skill":"control",
//! "stream":{"by":COMPUTER},"expires_in_ms":N}`) and publishes the computer's
//! sound on `speaker/audio` ([`Audio::SPEAKER_TOPIC`], 24 kHz mono S16LE,
//! source `control`). The voice answers one line, `{"event":"stream",
//! "state":"open","buffer_ms":N}` or a refusal in words; while the stream is
//! open it says `{"event":"stats",...}` once a second; the computer's door
//! ends the stream by closing the connection.
//!
//! This is the voice's half in two steps. This one reads the sound and counts
//! it (`blocks`, `lost`, `stray` in the stats line) and plays nothing: playing
//! is the speaker worker's, by the voice's own path, in the steps after. So it
//! is off unless `LAMP_SPEAKER_STREAM=on`; off, a stream is refused with the
//! words a voice from before it gives, "this lamp's voice does not take a
//! stream". One stream at a time, since the speaker is one device.
//!
//! Like a held mute ([`crate::control_mute`]) it lives on the client's thread
//! and is never a turn: nothing here waits on the voice or the voice on it.
use crate::say::Outcome;
use lamp_bus::{Bus, Subscriber};
use lamp_msgs::Audio;
use std::{
    io::{self, Read, Write},
    os::unix::net::UnixStream,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

/// The switch.
pub const ENVIRONMENT: &str = "LAMP_SPEAKER_STREAM";
/// Why a stream is refused where the voice does not take one (the words of a
/// voice from before the request, so a computer sees one thing).
pub const NOT_TAKEN: &str = "this lamp's voice does not take a stream";
/// Why a second stream is refused.
const BUSY: &str = "another computer's stream is playing";
/// What this reader is called to the bus.
const NAME: &str = "voice_speaker_stream";
/// The only publisher whose sound is taken: the paired computers' door.
const SOURCE: &str = "control";
/// The jitter buffer the voice will keep for a stream, as it tells the door.
const BUFFER_MS: u32 = 300;
/// How often the stream's counts are said.
pub const STATS_EVERY: Duration = Duration::from_secs(1);
/// How long one look at the connection waits, which paces the loop.
const LOOK: Duration = Duration::from_millis(20);
/// A computer that stops reading never holds this thread in a write.
const WRITE_WITHIN: Duration = Duration::from_millis(200);

/// Whether a stream is open on this lamp.
static OPEN: AtomicBool = AtomicBool::new(false);

/// What a stream has taken in so far.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Blocks of the right source, rate and shape.
    pub blocks: u64,
    /// Blocks the bus says went missing between ours.
    pub lost: u64,
    /// Blocks of another source or not mono 24 kHz S16LE, dropped.
    pub stray: u64,
}

/// Serve `by`'s stream on `stream`, the switch and bus as the process has
/// them.
pub fn serve(stream: UnixStream, by: &str, kept: Vec<u8>) {
    let on = std::env::var(ENVIRONMENT).is_ok_and(|value| value == "on");
    serve_with(stream, by, kept, on, Bus::from_environment, STATS_EVERY);
}

/// [`serve`], with the switch and the way to the bus given, and how often the
/// counts are said.
pub fn serve_with(
    mut stream: UnixStream,
    by: &str,
    _kept: Vec<u8>,
    on: bool,
    bus: impl FnOnce() -> io::Result<Bus>,
    stats_every: Duration,
) {
    let refuse = |stream: &mut UnixStream, why: &str| {
        let _ = writeln!(stream, "{}", Outcome::Refused(why.into()).line());
    };
    if !on {
        return refuse(&mut stream, NOT_TAKEN);
    }
    let Some(_open) = Open::claim() else {
        return refuse(&mut stream, BUSY);
    };
    let mut sound = match bus().and_then(|bus| Sound::on(&bus)) {
        Ok(sound) => sound,
        Err(error) => {
            return refuse(
                &mut stream,
                &format!("the voice could not read the sound: {error}"),
            );
        }
    };
    let _ = stream.set_write_timeout(Some(WRITE_WITHIN));
    if stream.set_read_timeout(Some(LOOK)).is_err()
        || writeln!(
            stream,
            r#"{{"event":"stream","state":"open","buffer_ms":{BUFFER_MS}}}"#
        )
        .is_err()
    {
        return;
    }
    eprintln!("[voice] speaker stream of {by:?} open");
    let mut last_said = Instant::now();
    let mut scrap = [0u8; 256];
    loop {
        // Gone: the stream goes with it, as its claim drops.
        match stream.read(&mut scrap) {
            Ok(0) => break,
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) => {}
            Err(_) => break,
        }
        if sound.take_in().is_err() {
            let _ = writeln!(
                stream,
                "{}",
                Outcome::Dropped("the voice lost the sound's bus".into()).line()
            );
            break;
        }
        if last_said.elapsed() >= stats_every {
            last_said = Instant::now();
            let counts = sound.counts;
            if writeln!(
                stream,
                r#"{{"event":"stats","buffered_ms":0,"underruns":0,"limited":0,"blocks":{},"lost":{},"stray":{}}}"#,
                counts.blocks, counts.lost, counts.stray
            )
            .is_err()
            {
                break;
            }
        }
    }
    eprintln!(
        "[voice] speaker stream of {by:?} closed: {:?}",
        sound.counts
    );
}

/// The one open stream, released when it is dropped.
struct Open;

impl Open {
    fn claim() -> Option<Self> {
        OPEN.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Self)
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        OPEN.store(false, Ordering::Release);
    }
}

/// `speaker/audio`, read for as long as a stream is open.
struct Sound {
    blocks: Subscriber,
    counts: Counts,
}

impl Sound {
    fn on(bus: &Bus) -> io::Result<Self> {
        Ok(Self {
            blocks: bus.subscribe_typed(Audio::SPEAKER_TOPIC, NAME, Audio::TYPE)?,
            counts: Counts::default(),
        })
    }

    /// Takes in every block the bus has now, none waited for.
    fn take_in(&mut self) -> io::Result<()> {
        while let Some(message) = self.blocks.recv(Duration::ZERO)? {
            let usable = message.header.source == SOURCE
                && Audio::split(&message.payload).is_ok_and(|(header, bytes)| {
                    header.sample_rate == Audio::SPEAKER_SAMPLE_RATE
                        && header.channels == 1
                        && header.sample_format == Audio::S16LE
                        && bytes.len() % 2 == 0
                });
            if usable {
                self.counts.blocks += 1;
                self.counts.lost += message.gap;
            } else {
                self.counts.stray += 1;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::sync::Mutex;
    use std::thread;

    /// One stream at a time is the lamp's, so these share it by turns.
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

    fn block(rate: u32, sequence: u64) -> Vec<u8> {
        Audio {
            sample_rate: rate,
            channels: 1,
            sample_format: Audio::S16LE.to_owned(),
            sequence,
        }
        .block(&[0u8; 480])
        .unwrap()
    }

    /// A voice serving one stream on one end of a pair, the other end handed
    /// back as a reader and the connection itself.
    fn serving(
        on: bool,
        bus: impl FnOnce() -> io::Result<Bus> + Send + 'static,
    ) -> (BufReader<UnixStream>, UnixStream, thread::JoinHandle<()>) {
        let (voice, computer) = UnixStream::pair().unwrap();
        let served = thread::spawn(move || {
            serve_with(
                voice,
                "desk",
                Vec::new(),
                on,
                bus,
                Duration::from_millis(50),
            );
        });
        let line_reader = BufReader::new(computer.try_clone().unwrap());
        (line_reader, computer, served)
    }

    fn line(reader: &mut BufReader<UnixStream>) -> serde_json::Value {
        reader
            .get_ref()
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut text = String::new();
        reader.read_line(&mut text).unwrap();
        serde_json::from_str(&text).unwrap_or_else(|_| panic!("a json line, got {text:?}"))
    }

    fn temp_bus(tag: &str) -> (Bus, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("lamp-speaker-stream-{tag}-{}", std::process::id()));
        (Bus::new(&dir).unwrap(), dir)
    }

    #[test]
    fn a_stream_is_refused_as_a_voice_without_it_does_while_the_switch_is_off() {
        let _turn = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (mut said, _computer, served) = serving(false, || panic!("the bus is not even opened"));
        assert_eq!(
            line(&mut said),
            serde_json::json!({"outcome": "refused", "why": NOT_TAKEN})
        );
        served.join().unwrap();
    }

    #[test]
    fn an_open_stream_counts_the_computers_blocks_and_the_rest_as_stray() {
        let _turn = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (bus, dir) = temp_bus("counts");
        let mut computers = bus
            .publisher_declared(Audio::SPEAKER_TOPIC, &Audio::declaration(), SOURCE)
            .unwrap();
        let mut other = bus
            .publisher_declared(Audio::SPEAKER_TOPIC, &Audio::declaration(), "someone")
            .unwrap();
        let served_bus = bus.clone();
        let (mut said, computer, served) = serving(true, move || Ok(served_bus));
        assert_eq!(
            line(&mut said),
            serde_json::json!({"event": "stream", "state": "open", "buffer_ms": 300})
        );
        for sequence in [1, 2, 4] {
            computers
                .publish_bytes(
                    lamp_ipc::monotonic_us(),
                    Audio::ENCODING,
                    &block(24_000, sequence),
                )
                .unwrap();
        }
        // Another publisher's block, and one at the microphone's rate.
        other
            .publish_bytes(lamp_ipc::monotonic_us(), Audio::ENCODING, &block(24_000, 1))
            .unwrap();
        computers
            .publish_bytes(lamp_ipc::monotonic_us(), Audio::ENCODING, &block(16_000, 5))
            .unwrap();
        let mut stats = serde_json::Value::Null;
        for _ in 0..40 {
            stats = line(&mut said);
            if stats["blocks"] == 3 && stats["stray"] == 2 {
                break;
            }
        }
        assert_eq!(stats["event"], "stats");
        assert_eq!(
            (stats["blocks"].as_u64(), stats["stray"].as_u64()),
            (Some(3), Some(2))
        );
        assert_eq!(stats["buffered_ms"], 0, "nothing is played yet");
        drop((said, computer));
        served.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_second_stream_is_refused_until_the_first_is_closed() {
        let _turn = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (bus, dir) = temp_bus("one");
        let open_bus = |bus: &Bus| {
            let bus = bus.clone();
            move || Ok(bus)
        };
        let (mut first, first_computer, first_served) = serving(true, open_bus(&bus));
        assert_eq!(line(&mut first)["state"], "open");
        let (mut second, _second_computer, second_served) = serving(true, open_bus(&bus));
        assert_eq!(
            line(&mut second),
            serde_json::json!({"outcome": "refused", "why": BUSY})
        );
        second_served.join().unwrap();
        // Closing the connection ends the stream, and the speaker is free.
        drop((first, first_computer));
        first_served.join().unwrap();
        let (mut third, _third_computer, third_served) = serving(true, open_bus(&bus));
        assert_eq!(line(&mut third)["state"], "open");
        drop((third, _third_computer));
        third_served.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
