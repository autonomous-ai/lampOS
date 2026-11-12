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
//! queued. A stream admitted while another runs replaces it, as the door's
//! own rule is (a later `on` replaces the running stream, and the door asks
//! the voice first, so a refusal leaves the old one be): the old one is
//! answered `dropped`, "replaced by a later stream". While open the voice
//! says `{"event":"stats",...}` once a second; the computer's door ends a
//! stream by closing the connection, which the voice answers `dropped`.
//!
//! The sound is read off the bus into a [`Feed`] the voice plays as a turn of
//! the lamp's own (`SpeechOrigin::ComputerStream`): the speaker takes it in
//! chunks of 240 samples after a prefill, bounded at 300 ms of held sound,
//! and the last chunk is faded when its computer closes the connection. The
//! stats line says `blocks`, `lost` and `stray` once a second. It is off unless
//! `LAMP_SPEAKER_STREAM=on`; off, a stream is refused with the words a voice
//! from before it gives, "this lamp's voice does not take a stream".
//!
//! The connection's thread is the client's, never the voice's: nothing here
//! waits on the voice's loop or the voice on it.
use crate::say::{Answer, Cancel, Outcome, is_cancel};
use lamp_bus::{Bus, Subscriber};
use lamp_msgs::Audio;
use std::{
    collections::VecDeque,
    io::{self, Read, Write},
    os::unix::net::UnixStream,
    sync::{
        Arc, Mutex,
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

/// The service started with the switch `on` or not.
pub fn set(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

/// Whether the voice takes streams.
pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// The speaker's chunk, in samples: 10 ms at 24 kHz, as a lamp's own sound is
/// handed to the speaker worker.
pub const CHUNK: usize = 240;
/// The sound held before the first chunk goes, so a block late by a few
/// milliseconds is not heard as a gap: 120 ms.
const PREFILL: usize = 2_880;
/// The most sound held for the speaker, the jitter buffer the door is told
/// of ([`BUFFER_MS`]): 300 ms. Sound past it is dropped, newest first, and
/// counted: a computer that sends faster than real time loses sound, never
/// the lamp's memory.
const MOST_HELD: usize = 7_200;

/// A stream's sound on its way to the speaker: the client's thread puts what
/// it reads off the bus in, the voice's speaker handoff takes it out in
/// [`CHUNK`]s ([`crate::coordinator`]'s sound path). Closed, what is left
/// plays out and ends with one chunk faded to nothing.
#[derive(Debug, Default)]
pub struct Feed {
    state: Mutex<Held>,
}

#[derive(Debug, Default)]
struct Held {
    samples: VecDeque<i16>,
    closed: bool,
    started: bool,
    finished: bool,
    dropped: u64,
}

/// A feed is its own: two are equal only if they are the same one.
impl PartialEq for Feed {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl Eq for Feed {}

impl Feed {
    pub fn new() -> Arc<Self> {
        Arc::default()
    }

    fn held(&self) -> std::sync::MutexGuard<'_, Held> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Sound read off the bus; what does not fit is dropped and counted.
    pub fn push(&self, block: &[i16]) {
        let mut held = self.held();
        let room = MOST_HELD.saturating_sub(held.samples.len());
        held.samples.extend(&block[..block.len().min(room)]);
        held.dropped += block.len().saturating_sub(room) as u64;
    }

    /// No more sound comes: what is held plays out.
    pub fn close(&self) {
        self.held().closed = true;
    }

    /// Whether a chunk may go to the speaker now: a whole one is held (the
    /// first only after the prefill), or the stream is closed and its end
    /// has not gone yet.
    pub fn ready(&self) -> bool {
        let held = self.held();
        if held.finished {
            return false;
        }
        held.closed || held.samples.len() >= if held.started { CHUNK } else { PREFILL }
    }

    /// The next chunk and whether it is the last: always [`CHUNK`] samples,
    /// padded with silence at the end, and the last faded to nothing.
    pub fn take(&self) -> Option<(Vec<i16>, bool)> {
        let mut held = self.held();
        let ready = !held.finished
            && (held.closed || held.samples.len() >= if held.started { CHUNK } else { PREFILL });
        if !ready {
            return None;
        }
        let count = held.samples.len().min(CHUNK);
        let mut chunk: Vec<i16> = held.samples.drain(..count).collect();
        chunk.resize(CHUNK, 0);
        held.started = true;
        let last = held.closed && held.samples.is_empty();
        if last {
            held.finished = true;
            for (at, sample) in chunk.iter_mut().enumerate() {
                *sample = (i32::from(*sample) * (CHUNK - at) as i32 / CHUNK as i32) as i16;
            }
        }
        Some((chunk, last))
    }

    /// Whether the last chunk has gone.
    pub fn finished(&self) -> bool {
        self.held().finished
    }

    /// Sound dropped for want of room.
    pub fn dropped(&self) -> u64 {
        self.held().dropped
    }
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
    by: String,
    already: Vec<u8>,
) {
    hold_with(stream, answered, cancel, by, already, bus, STATS_EVERY);
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
    by: String,
    already: Vec<u8>,
    bus: impl FnOnce() -> io::Result<Bus>,
    stats_every: Duration,
) {
    let _ = stream.set_write_timeout(Some(WRITE_WITHIN));
    let mut bus = Some(bus);
    let mut feed: Option<Arc<Feed>> = None;
    let mut sound: Option<Sound> = None;
    let mut last_said = Instant::now();
    let mut closed = false;
    let mut stopped = false;
    // What the computer wrote after its request line, in the read that took it.
    let mut said_so_far = Said {
        held: already,
        dropping: false,
    };
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
                        eprintln!("[voice] speaker stream of {:?} open", by);
                    }
                    // The voice has the speaker's turn; it ends at once, in
                    // its own words, and its ticket with it.
                    Err(error) => {
                        eprintln!("[voice] speaker stream: no bus for the sound: {error}");
                        cancel.store(true, Ordering::Release);
                        break Outcome::Dropped("the voice could not read the sound".into());
                    }
                }
            }
            Ok(Answer::Stream(taken)) => {
                // Its computer may have closed before the feed came.
                if closed {
                    taken.close();
                }
                feed = Some(taken);
            }
            Ok(Answer::Done(outcome)) => break outcome,
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                break Outcome::Dropped("the voice has gone".into());
            }
        }
        // Its computer closing the connection ends the sound, not cuts it:
        // the feed is closed with no cancel, so what is held plays out, its
        // last chunk faded, and the voice answers `spoken` (a cancel would
        // cut the turn at once, the held sound dropped).
        if !closed {
            match asked(&stream, &mut said_so_far) {
                Asked::Nothing => {}
                Asked::Closed => {
                    closed = true;
                    if let Some(feed) = feed.as_ref() {
                        feed.close();
                    }
                }
                // Its hard stop: the cancel cuts the turn at once and the
                // voice answers `dropped`, as it does for any say's asker.
                Asked::Stop => {
                    closed = true;
                    stopped = true;
                    cancel.store(true, Ordering::Release);
                    if let Some(feed) = feed.as_ref() {
                        feed.close();
                    }
                }
            }
        }
        if let Some(sound) = sound.as_mut() {
            // Blocks that come after the close are not played.
            if sound.take_in(feed.as_deref().filter(|_| !closed)).is_err() {
                cancel.store(true, Ordering::Release);
                break Outcome::Dropped("the voice lost the sound's bus".into());
            } else if !closed && last_said.elapsed() >= stats_every {
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
    // The journal says a cut from a close: the same counts, and why.
    eprintln!(
        "[voice] speaker stream of {:?} closed{}: {:?}",
        by,
        if stopped {
            " (stopped by its asker)"
        } else {
            ""
        },
        sound.map(|sound| sound.counts)
    );
}

/// What its computer did on the stream's connection since the last look.
#[derive(Debug, PartialEq, Eq)]
enum Asked {
    /// Nothing that ends the stream.
    Nothing,
    /// The connection closed: the sound plays out and fades.
    Closed,
    /// A `{"cancel":true}` line, its hard stop (the door's `off`, `stop` or a
    /// revoke): the sound is cut at once, as a say's cancel cuts it.
    Stop,
}

/// What its computer wrote that is not yet a whole line.
struct Said {
    held: Vec<u8>,
    /// Past the most a line may be: what comes is dropped up to its newline,
    /// so the tail of an over-long line is never read as a line of its own.
    dropping: bool,
}

/// Reads what its computer wrote without waiting, whole lines kept in `said`.
/// A line is its stop when it is [`crate::say::CANCEL_LINE`]'s; any other line
/// is dropped; a line past [`MOST_LINE`] is dropped whole, as it comes. A look
/// reads at most [`MOST_READS`] pieces, so a computer that never stops writing
/// cannot hold this thread in it.
fn asked(stream: &UnixStream, said: &mut Said) -> Asked {
    if stream.set_nonblocking(true).is_err() {
        return Asked::Nothing;
    }
    let mut piece = [0u8; 512];
    let mut found = Asked::Nothing;
    for _ in 0..MOST_READS {
        match (&*stream).read(&mut piece) {
            Ok(0) => {
                found = Asked::Closed;
                break;
            }
            Ok(read) => {
                let mut fresh = &piece[..read];
                if said.dropping {
                    match fresh.iter().position(|&byte| byte == b'\n') {
                        Some(end) => {
                            said.dropping = false;
                            fresh = &fresh[end + 1..];
                        }
                        None => continue,
                    }
                }
                said.held.extend_from_slice(fresh);
                if said.held.len() > MOST_LINE && !said.held.contains(&b'\n') {
                    said.held.clear();
                    said.dropping = true;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => {
                if error.kind() != io::ErrorKind::WouldBlock {
                    found = Asked::Closed;
                }
                break;
            }
        }
    }
    let _ = stream.set_nonblocking(false);
    while let Some(end) = said.held.iter().position(|&byte| byte == b'\n') {
        let line: Vec<u8> = said.held.drain(..=end).collect();
        if is_cancel(&line) {
            return Asked::Stop;
        }
    }
    found
}

/// The most pieces one look reads.
const MOST_READS: usize = 64;

/// The most of a line from its computer that is kept waiting for its newline.
const MOST_LINE: usize = 4096;

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

    /// Takes in every block the bus has now, none waited for, into `feed`.
    fn take_in(&mut self, feed: Option<&Feed>) -> io::Result<()> {
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
                if let (Some(feed), Ok((_, bytes))) = (feed, Audio::split(&message.payload)) {
                    let samples: Vec<i16> = bytes
                        .chunks_exact(2)
                        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
                        .collect();
                    feed.push(&samples);
                }
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
    ) -> (
        mpsc::Sender<Answer>,
        Cancel,
        BufReader<UnixStream>,
        UnixStream,
        thread::JoinHandle<()>,
    ) {
        held_after(bus, Vec::new())
    }

    /// [`held`], with `already` the bytes that came with the request line.
    #[allow(clippy::type_complexity)]
    fn held_after(
        bus: Bus,
        already: Vec<u8>,
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
                "desk".to_owned(),
                already,
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
        let (answer, _cancel, mut said, computer, served) = held(bus);
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
    fn a_closed_connection_closes_the_feed_without_a_cancel_so_what_is_held_plays_out() {
        let _turn = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (bus, dir) = temp_bus("close");
        let (answer, cancel, mut said, computer, served) = held(bus);
        let feed = Feed::new();
        answer.send(Answer::Begun(1)).unwrap();
        answer.send(Answer::Stream(Arc::clone(&feed))).unwrap();
        assert_eq!(line(&mut said)["state"], "open");
        feed.push(&vec![1000i16; 500]);
        drop((said, computer));
        let began = Instant::now();
        while !feed.ready() {
            assert!(
                began.elapsed() < Duration::from_secs(2),
                "the close was not seen"
            );
            thread::sleep(Duration::from_millis(5));
        }
        // Closed, not cancelled: a cancel would cut the turn and drop what
        // is held, with no fade, and the answer would never be `spoken`.
        assert!(!cancel.load(Ordering::Acquire));
        // The stats line is not written to the closed socket: several ticks
        // (50 ms here) later it is still no cancel.
        thread::sleep(Duration::from_millis(400));
        assert!(!cancel.load(Ordering::Acquire));
        // All 500 held samples come out as chunks, the last one faded.
        let mut chunks = 0;
        while let Some((_, last)) = feed.take() {
            chunks += 1;
            if last {
                break;
            }
        }
        assert_eq!(chunks, 3);
        assert!(feed.finished());
        // The voice answers once it has played out; the thread ends.
        answer.send(Answer::Done(Outcome::Spoken)).unwrap();
        served.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_cancel_line_is_the_computers_hard_stop_and_other_lines_are_not() {
        let _turn = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (bus, dir) = temp_bus("hard-stop");
        let (answer, cancel, mut said, mut computer, served) = held(bus);
        answer.send(Answer::Begun(1)).unwrap();
        assert_eq!(line(&mut said)["state"], "open");
        // Lines that are no cancel (and a cancel that is not `true`) are dropped,
        // the stream runs on.
        computer
            .write_all(b"{\"hello\":1}\n{\"cancel\":false}\nnot json\n")
            .unwrap();
        thread::sleep(Duration::from_millis(300));
        assert!(!cancel.load(Ordering::Acquire), "no stop was asked");
        // The stop arrives in two pieces: still one line, still its stop.
        computer.write_all(b"{\"can").unwrap();
        thread::sleep(Duration::from_millis(100));
        assert!(!cancel.load(Ordering::Acquire));
        computer.write_all(b"cel\":true}\n").unwrap();
        let began = Instant::now();
        while !cancel.load(Ordering::Acquire) {
            assert!(
                began.elapsed() < Duration::from_secs(2),
                "the stop was not seen"
            );
            thread::sleep(Duration::from_millis(5));
        }
        // The voice cuts the turn and answers; the thread ends.
        answer
            .send(Answer::Done(Outcome::Dropped(
                crate::say::STOPPED_BY_ASKER.into(),
            )))
            .unwrap();
        served.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_stop_that_came_with_the_request_line_is_its_stop_and_an_over_long_lines_tail_is_not() {
        let _turn = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Bytes that arrived in the same read as the request line count.
        let (bus, dir) = temp_bus("stop-with-request");
        let (answer, cancel, _said, _computer, served) =
            held_after(bus, b"{\"cancel\":true}\n".to_vec());
        let began = Instant::now();
        while !cancel.load(Ordering::Acquire) {
            assert!(
                began.elapsed() < Duration::from_secs(2),
                "the stop that came with the request was lost"
            );
            thread::sleep(Duration::from_millis(5));
        }
        answer
            .send(Answer::Done(Outcome::Dropped("stopped".into())))
            .unwrap();
        served.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
        // A line past the most is dropped whole: its tail, which here is a
        // cancel line, is no line of its own; the next real one is the stop.
        let (bus, dir) = temp_bus("long-line");
        let (answer, cancel, _said, mut computer, served) = held(bus);
        for _ in 0..12 {
            computer.write_all(&[b'a'; 512]).unwrap();
        }
        thread::sleep(Duration::from_millis(200));
        computer.write_all(b"{\"cancel\":true}\n").unwrap();
        thread::sleep(Duration::from_millis(300));
        assert!(
            !cancel.load(Ordering::Acquire),
            "the tail of an over-long line was read as a stop"
        );
        computer.write_all(b"{\"cancel\":true}\n").unwrap();
        let began = Instant::now();
        while !cancel.load(Ordering::Acquire) {
            assert!(
                began.elapsed() < Duration::from_secs(2),
                "the stop after it was not seen"
            );
            thread::sleep(Duration::from_millis(5));
        }
        answer
            .send(Answer::Done(Outcome::Dropped("stopped".into())))
            .unwrap();
        served.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_feed_that_comes_after_the_close_is_closed_too() {
        let _turn = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (bus, dir) = temp_bus("late-feed");
        let (answer, cancel, said, computer, served) = held(bus);
        drop((said, computer));
        thread::sleep(Duration::from_millis(300));
        let feed = Feed::new();
        answer.send(Answer::Stream(Arc::clone(&feed))).unwrap();
        let began = Instant::now();
        while !feed.ready() {
            assert!(
                began.elapsed() < Duration::from_secs(2),
                "the late feed was not closed"
            );
            thread::sleep(Duration::from_millis(5));
        }
        assert!(!cancel.load(Ordering::Acquire));
        answer.send(Answer::Done(Outcome::Spoken)).unwrap();
        served.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_feed_holds_the_prefill_chunks_what_fits_and_ends_faded_at_its_close() {
        let feed = Feed::new();
        // Under the prefill nothing goes, not even a whole chunk.
        feed.push(&vec![1000i16; PREFILL - 1]);
        assert!(!feed.ready() && feed.take().is_none());
        feed.push(&[1000]);
        assert!(feed.ready());
        // Whole chunks of 240 samples, none last while the stream is open.
        let (chunk, last) = feed.take().unwrap();
        assert_eq!((chunk.len(), last, chunk[0]), (CHUNK, false, 1000));
        // Past the most held, the rest is dropped and counted.
        feed.push(&vec![1000i16; MOST_HELD]);
        assert!(feed.dropped() > 0);
        // Once started, a single chunk is enough; fewer than one waits.
        let held = {
            let mut taken = 0;
            while feed.take().is_some_and(|(_, last)| !last) {
                taken += 1;
                if !feed.ready() {
                    break;
                }
            }
            taken
        };
        assert!(held > 0);
        feed.push(&[500; 100]);
        assert!(
            !feed.ready(),
            "a part of a chunk waits while the stream is open"
        );
        // Closed, the rest plays out, padded, the last chunk faded to nothing.
        feed.close();
        let (chunk, last) = feed.take().unwrap();
        assert!(last && feed.finished());
        assert_eq!(chunk.len(), CHUNK);
        assert!(chunk[0] > 0 && chunk[99] > 0 && chunk[CHUNK - 1] == 0);
        assert!(
            !feed.ready() && feed.take().is_none(),
            "nothing after the last"
        );
    }
}
