//! A paired computer's sound for the speaker, taken in off the bus while a
//! stream of its is open (`speaker.stream`, docs/control.md "The speaker").
//!
//! lamp-control opens a stream on the say socket (`{"skill":"control",
//! "stream":{"by":COMPUTER},"expires_in_ms":N}`) and publishes the computer's
//! sound on `speaker/audio` ([`Audio::SPEAKER_TOPIC`], 24 kHz mono S16LE,
//! source `control`). The stream is a say the voice admits like an exact line
//! ([`crate::say::Says::admit_stream`]): at rest it is `begun` and the
//! computer is told `{"event":"stream","state":"open","buffer_ms":N}`; in a
//! conversation, speaking, muted or paused it is refused in words, never
//! queued; a second is refused. While open the voice says
//! `{"event":"stats",...}` once a second; the computer's door ends the
//! stream by closing the connection, which the voice answers `dropped`.
//!
//! This step reads the sound and counts it (`blocks`, `lost`, `stray` in the
//! stats line) and plays nothing: playing is the speaker worker's, by the
//! voice's own path, in the step after. So it is off unless
//! `LAMP_SPEAKER_STREAM=on`; off, a stream is refused with the words a voice
//! from before it gives, "this lamp's voice does not take a stream".
//!
//! The connection's thread is the client's, never the voice's: nothing here
//! waits on the voice's loop or the voice on it.
use crate::say::{Answer, Cancel, Outcome, asker_went};
use lamp_bus::{Bus, Subscriber};
use lamp_msgs::Audio;
use std::{
    io::{self, Write},
    os::unix::net::UnixStream,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

/// The switch.
pub const ENVIRONMENT: &str = "LAMP_SPEAKER_STREAM";
/// Why a stream is refused where the voice does not take one (the words of a
/// voice from before the request, so a computer sees one thing).
pub const NOT_TAKEN: &str = "this lamp's voice does not take a stream";
/// Why a second stream is refused.
pub const BUSY: &str = "another computer's stream is playing";
/// What this reader is called to the bus.
const NAME: &str = "voice_speaker_stream";
/// The only publisher whose sound is taken: the paired computers' door.
const SOURCE: &str = "control";
/// The jitter buffer the voice will keep for a stream, as it tells the door.
const BUFFER_MS: u32 = 300;
/// How often the stream's counts are said.
pub const STATS_EVERY: Duration = Duration::from_secs(1);
/// How long one look at the voice's answer waits, which paces the loop.
const LOOK: Duration = Duration::from_millis(20);
/// A computer that stops reading never holds this thread in a write.
const WRITE_WITHIN: Duration = Duration::from_millis(200);

/// Whether [`ENVIRONMENT`] was `on` when the service started.
static ON: AtomicBool = AtomicBool::new(false);
/// Whether a stream is open on this lamp.
static OPEN: AtomicBool = AtomicBool::new(false);

/// The service started with the switch `on` or not.
pub fn set(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

/// Whether the voice takes streams.
pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

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

/// Holds `stream` for its computer while the voice decides and then for as
/// long as the stream is open: the voice's answers come on `answered`
/// (`begun` opens it, a final outcome ends it), and a closed connection sets
/// `cancel`, which the voice reads as the end.
pub(crate) fn hold(
    stream: UnixStream,
    answered: mpsc::Receiver<Answer>,
    cancel: Cancel,
    claim: Open,
) {
    hold_with(stream, answered, cancel, claim, bus, STATS_EVERY);
}

/// The lamp's bus; a test's own, when it has one.
fn bus() -> io::Result<Bus> {
    #[cfg(test)]
    if let Some(bus) = TEST_BUS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
    {
        return Ok(bus);
    }
    Bus::from_environment()
}

/// A test's bus for [`bus`], and the lock that gives the one stream the lamp
/// takes to one test at a time.
#[cfg(test)]
pub(crate) static TEST_BUS: std::sync::Mutex<Option<Bus>> = std::sync::Mutex::new(None);
#[cfg(test)]
pub(crate) static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// [`hold`], with the way to the bus given and how often the counts are said.
pub(crate) fn hold_with(
    mut stream: UnixStream,
    answered: mpsc::Receiver<Answer>,
    cancel: Cancel,
    claim: Open,
    bus: impl FnOnce() -> io::Result<Bus>,
    stats_every: Duration,
) {
    let _ = stream.set_write_timeout(Some(WRITE_WITHIN));
    let mut bus = Some(bus);
    let mut sound: Option<Sound> = None;
    let mut last_said = Instant::now();
    let outcome = loop {
        match answered.recv_timeout(LOOK) {
            Ok(Answer::Begun(_)) => {
                let opened = bus
                    .take()
                    .map(|bus| bus().and_then(|bus| Sound::on(&bus)))
                    .unwrap_or_else(|| Err(io::Error::other("opened twice")));
                match opened {
                    Ok(taken) => {
                        sound = Some(taken);
                        last_said = Instant::now();
                        if writeln!(
                            stream,
                            r#"{{"event":"stream","state":"open","buffer_ms":{BUFFER_MS}}}"#
                        )
                        .is_err()
                        {
                            cancel.store(true, Ordering::Release);
                        }
                        eprintln!("[voice] speaker stream of {:?} open", claim.by);
                    }
                    // The voice has the speaker's turn; it ends at once.
                    Err(error) => {
                        eprintln!("[voice] speaker stream: no bus for the sound: {error}");
                        cancel.store(true, Ordering::Release);
                    }
                }
            }
            Ok(Answer::Done(outcome)) => break outcome,
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                break Outcome::Dropped("the voice has gone".into());
            }
        }
        if !cancel.load(Ordering::Acquire) && asker_went(&stream) {
            cancel.store(true, Ordering::Release);
        }
        if let Some(sound) = sound.as_mut() {
            if sound.take_in().is_err() {
                cancel.store(true, Ordering::Release);
            } else if last_said.elapsed() >= stats_every {
                last_said = Instant::now();
                let counts = sound.counts;
                if writeln!(
                    stream,
                    r#"{{"event":"stats","buffered_ms":0,"underruns":0,"limited":0,"blocks":{},"lost":{},"stray":{}}}"#,
                    counts.blocks, counts.lost, counts.stray
                )
                .is_err()
                {
                    cancel.store(true, Ordering::Release);
                }
            }
        }
    };
    let _ = writeln!(stream, "{}", outcome.line());
    eprintln!(
        "[voice] speaker stream of {:?} closed: {:?}",
        claim.by,
        sound.map(|sound| sound.counts)
    );
}

/// The one open stream, released when it is dropped.
pub(crate) struct Open {
    by: String,
}

impl Open {
    pub(crate) fn claim(by: &str) -> Option<Self> {
        OPEN.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Self { by: by.to_owned() })
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
    use std::thread;

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

    /// The voice's side of one stream, held on one end of a pair: its answers
    /// go in through the sender, the other end comes back as a reader.
    #[allow(clippy::type_complexity)]
    fn held(
        bus: Bus,
        claim: Open,
    ) -> (
        mpsc::Sender<Answer>,
        Cancel,
        BufReader<UnixStream>,
        UnixStream,
        thread::JoinHandle<()>,
    ) {
        let (voice, computer) = UnixStream::pair().unwrap();
        let (answer, answered) = mpsc::channel();
        let cancel = Cancel::default();
        let held_cancel = Cancel::clone(&cancel);
        let served = thread::spawn(move || {
            hold_with(
                voice,
                answered,
                held_cancel,
                claim,
                move || Ok(bus),
                Duration::from_millis(50),
            );
        });
        let reader = BufReader::new(computer.try_clone().unwrap());
        (answer, cancel, reader, computer, served)
    }

    #[test]
    fn an_open_stream_counts_the_computers_blocks_and_the_rest_as_stray() {
        let _turn = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (bus, dir) = temp_bus("counts");
        let mut computers = bus
            .publisher_declared(Audio::SPEAKER_TOPIC, &Audio::declaration(), SOURCE)
            .unwrap();
        let mut other = bus
            .publisher_declared(Audio::SPEAKER_TOPIC, &Audio::declaration(), "someone")
            .unwrap();
        let (answer, _cancel, mut said, computer, served) = held(bus, Open::claim("desk").unwrap());
        // Nothing is said until the voice has begun it.
        answer.send(Answer::Begun(1)).unwrap();
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
        answer
            .send(Answer::Done(Outcome::Dropped(
                "the lamp has something to say".into(),
            )))
            .unwrap();
        let mut ended = serde_json::Value::Null;
        for _ in 0..40 {
            ended = line(&mut said);
            if ended["outcome"].is_string() {
                break;
            }
        }
        assert_eq!(
            ended,
            serde_json::json!({"outcome": "dropped", "why": "the lamp has something to say"})
        );
        served.join().unwrap();
        drop(computer);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_closed_connection_is_the_computers_cancel_and_the_voice_ends_it() {
        let _turn = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (bus, dir) = temp_bus("close");
        let (answer, cancel, mut said, computer, served) = held(bus, Open::claim("desk").unwrap());
        answer.send(Answer::Begun(1)).unwrap();
        assert_eq!(line(&mut said)["state"], "open");
        drop((said, computer));
        let began = Instant::now();
        while !cancel.load(Ordering::Acquire) {
            assert!(
                began.elapsed() < Duration::from_secs(2),
                "the close was not seen"
            );
            thread::sleep(Duration::from_millis(5));
        }
        // The voice reads the cancel and answers; the thread ends.
        answer
            .send(Answer::Done(Outcome::Dropped("closed".into())))
            .unwrap();
        served.join().unwrap();
        // The speaker is free again once the hold is gone.
        assert!(Open::claim("laptop").is_some());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_second_claim_is_refused_until_the_first_is_dropped() {
        let _turn = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let first = Open::claim("desk").unwrap();
        assert!(Open::claim("laptop").is_none());
        drop(first);
        assert!(Open::claim("laptop").is_some());
    }
}
