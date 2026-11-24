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
// Serve sets this flag when starting the say service; its client admission
// reads the local stream switch.
static ON: AtomicBool = AtomicBool::new(false);

/// The service started with the switch `on` or not.
pub fn set(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

/// Whether the voice takes streams.
pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// The switch for the long-form bound (`on` exactly; off unless set).
pub const LONG_FORM_ENVIRONMENT: &str = "LAMP_STREAM_SPEECH_LEVEL";

/// Whether [`LONG_FORM_ENVIRONMENT`] was `on` when the service started.
// Serve initializes this flag for the say service; stream handling in that
// process reads the long-form bound switch.
static LONG_FORM: AtomicBool = AtomicBool::new(false);

/// The service started with the long-form switch `on` or not.
pub fn set_long_form(on: bool) {
    LONG_FORM.store(on, Ordering::Relaxed);
}

/// Whether a stream is held to the long-form bound.
pub fn long_form_on() -> bool {
    LONG_FORM.load(Ordering::Relaxed)
}

/// The switch for the long-form gain (`on` exactly; off unless set). It lifts only a
/// long-form stream, so it does nothing without [`LONG_FORM_ENVIRONMENT`].
pub const GAIN_ENVIRONMENT: &str = "LAMP_LONG_FORM_GAIN";

/// Whether [`GAIN_ENVIRONMENT`] was `on` when the service started.
// Serve initializes this flag for the say service; stream handling in that
// process reads the long-form gain switch.
static GAIN: AtomicBool = AtomicBool::new(false);

/// The service started with the gain switch `on` or not.
pub fn set_gain(on: bool) {
    GAIN.store(on, Ordering::Relaxed);
}

/// Whether a long-form stream is lifted toward the gain's target.
pub fn gain_on() -> bool {
    GAIN.load(Ordering::Relaxed)
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
///
/// A long exact line's pieces (#7892, [`crate::say::tts_cache::Pieces`])
/// come through a feed of their own size ([`Feed::for_line`]): its
/// renderer puts each piece in as it is rendered, at most one ahead of what
/// plays, and stops once the line is let go ([`Feed::abandon`]).
#[derive(Debug)]
pub struct Feed {
    state: Mutex<Held>,
    /// The most sound held: [`MOST_HELD`] for a stream.
    most: usize,
}

#[derive(Debug, Default)]
struct Held {
    samples: VecDeque<i16>,
    closed: bool,
    started: bool,
    finished: bool,
    dropped: u64,
    /// Gaps the speaker reported in this stream's sound.
    underruns: u64,
    /// Nobody plays it any more: its line was cut, refused or let go.
    abandoned: bool,
    /// Closed before all its sound came: a piece of a line not rendered.
    short: bool,
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
        Self::for_line(MOST_HELD)
    }

    /// A feed that holds up to `most` samples: a long exact line's, whose
    /// renderer keeps it to the piece playing and the next one.
    pub fn for_line(most: usize) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::default(),
            most,
        })
    }

    fn held(&self) -> std::sync::MutexGuard<'_, Held> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Sound read off the bus; what does not fit is dropped and counted.
    pub fn push(&self, block: &[i16]) {
        let mut held = self.held();
        let room = self.most.saturating_sub(held.samples.len());
        held.samples.extend(&block[..block.len().min(room)]);
        held.dropped += block.len().saturating_sub(room) as u64;
    }

    /// No more sound comes: what is held plays out.
    pub fn close(&self) {
        self.held().closed = true;
    }

    /// No more sound comes, though some was owed: what is held plays out,
    /// and the line is not said whole ([`Feed::short`]).
    pub fn close_short(&self) {
        let mut held = self.held();
        held.closed = true;
        held.short = true;
    }

    /// Whether it was closed before all its sound came.
    pub fn short(&self) -> bool {
        self.held().short
    }

    /// Nobody plays it any more: its renderer stops.
    pub fn abandon(&self) {
        self.held().abandoned = true;
    }

    /// Whether nobody plays it any more.
    pub fn abandoned(&self) -> bool {
        self.held().abandoned
    }

    /// The samples held now, not yet taken.
    pub fn held_samples(&self) -> usize {
        self.held().samples.len()
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

    /// The speaker reported a gap in this stream's sound: its queue ran dry
    /// while the stream played (a playback gap that started), counted.
    pub fn underrun(&self) {
        self.held().underruns += 1;
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

    /// Gaps in the sound the speaker reported (its own queue ran dry), counted
    /// by [`Feed::underrun`].
    pub fn underruns(&self) -> u64 {
        self.held().underruns
    }

    /// The sound held now, in milliseconds of the speaker's 24 kHz.
    pub fn buffered_ms(&self) -> u64 {
        self.held().samples.len() as u64 / 24
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
    /// The 10 ms windows of sound the loudness limit lowered or clipped.
    pub limited: u64,
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
// The say test installs and clears this shared test-process bus; speaker_stream::bus
// reads the same value for its stream.
pub(crate) static TEST_BUS: std::sync::Mutex<Option<Bus>> = std::sync::Mutex::new(None);
#[cfg(test)]
// Stream tests acquire and release this shared test-process mutex around their stream
// fixtures.
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
                    r#"{{"event":"stats","buffered_ms":{},"underruns":{},"limited":{},"blocks":{},"lost":{},"stray":{}}}"#,
                    feed.as_ref().map_or(0, |feed| feed.buffered_ms()),
                    feed.as_ref().map_or(0, |feed| feed.underruns()),
                    counts.limited, counts.blocks, counts.lost, counts.stray
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

/// The loudness a computer's stream is held to. Without
/// [`LONG_FORM_ENVIRONMENT`] it is the bound a skill's short sound has
/// ([`crate::say::SOUND_PEAK_DBFS`], [`crate::say::SOUND_LOUDEST_DBFS`]): no
/// sample past the first and no 10 ms louder than the second as RMS. A clip
/// past it is refused; a stream, which cannot be read ahead, is lowered.
///
/// With the switch, a stream is *long-form*: a computer's or app's audio on
/// `speaker.stream`, a story, music or a call, which runs for as long as the
/// computer sends and is the whole of what a person came to hear. It is held
/// to [`LONG_FORM_LOUDEST_DBFS`] RMS and [`LONG_FORM_PEAK_DBFS`] instead. A
/// skill's `sound` op (a clip of at most 3 s, a chime) is not long-form and
/// keeps the short bound, which this does not touch. The owner's volume and
/// the night caps act after the voice's samples (the speaker's mixer) and are
/// not changed by either bound.
const LIMIT_WINDOW: usize = 240;

/// The long-form RMS bound: the lamp's own speech sits at about -20 dBFS, so
/// a story plays about as loud as the voice, not 10 dB under it.
pub const LONG_FORM_LOUDEST_DBFS: f64 = -20.0;

/// The long-form peak bound. Speech and recorded stories have peaks 10 to 14
/// dB over their RMS; a peak bound only 8 dB over the RMS bound (-12) would
/// clip every one of them and make a story harsh. -6 dBFS leaves 14 dB over
/// -20 and still keeps every sample 6 dB under full scale, so a train of
/// full-scale clicks is still held.
pub const LONG_FORM_PEAK_DBFS: f64 = -6.0;

/// The long-form bounds with the gain on. The per-window RMS bound is -10
/// dBFS, not the -20 of a stream without the gain and not off: speech has
/// words 10 dB over its average, so a -17 cap undoes the lift on exactly the
/// loud words (by hand on the person's excerpt: -17 cut 966 of 3000 windows
/// and left the whole RMS at the source's -21), while no bound at all lets a
/// stream that turns loud play at its own level until the slow gain has come
/// down (Heron on #8399). -10 sits over the person's loudest words with the
/// gain (about -9 by hand, a few windows cut by less than 1 dB) and 10 dB over
/// what the gain is aiming for. The peak is held at -1, as in the by-hand
/// files the person picked.
pub const LIFTED_LOUDEST_DBFS: f64 = -10.0;
pub const LIFTED_PEAK_DBFS: f64 = -1.0;

/// The RMS the limiter aims at, a part of the cap: 2% (0.18 dB) under it.
const RMS_MARGIN: f64 = 0.98;

/// The peak the limiter aims at, a part of the bound: 0.1% under it.
const PEAK_MARGIN: f64 = 0.999;

/// The level the long-form gain brings a quiet stream up to, dBFS RMS of the
/// windows that are not pauses. The person's pick on the by-hand files was the
/// source +3 dB, whole-file RMS -18; the whole file counts its pauses, which
/// the level here does not, so on that excerpt (active level about -20) the
/// same +3 dB is this target.
pub const GAIN_TARGET_DBFS: f64 = -17.0;

/// The most the gain adds, dB.
pub const GAIN_MOST_DB: f64 = 6.0;

/// The most the gain takes off, dB: a stream louder than the target is brought
/// down to it, at the same slow rates, so a loud stream is not louder with the
/// gain on than the target says (Heron on #8399).
pub const GAIN_LEAST_DB: f64 = -20.0;

/// How fast the gain rises, dB per 10 ms window (3 dB a second), and how fast
/// it falls when the stream gets louder (6 dB a second): slow, so a changed
/// level is followed over seconds and no word is pumped.
const GAIN_UP_DB: f64 = 0.03;
const GAIN_DOWN_DB: f64 = 0.06;

/// The level estimate's time constant, in windows (4 s).
const GAIN_LEVEL_WINDOWS: f64 = 400.0;

/// A window under this RMS (dBFS) is a pause: it does not move the level
/// estimate or the gain, so a pause never makes the next word louder, and a
/// quiet intro or room tone (a -40 breath) neither seeds the gain nor gives it
/// the full lift before the first speech.
const GAIN_PAUSE_DBFS: f64 = -36.0;

/// The long-form gain: a slow follower of a stream's own level. It keeps a
/// running mean square of the windows that are not pauses (4 s time
/// constant) and wants the gain that brings it to [`GAIN_TARGET_DBFS`],
/// between [`GAIN_LEAST_DB`] and [`GAIN_MOST_DB`] dB. The first block that has
/// anything that is not a pause sets the level and the
/// gain at once, so a stream does not swell in (a stream louder than the
/// target is brought down from its first block); after that the gain moves at
/// most [`GAIN_UP_DB`] up or [`GAIN_DOWN_DB`] down per window. Nothing is
/// read ahead and no sound is held back. It only computes the factor: the
/// [`Limiter`] applies it before it measures the window, so a lifted peak is
/// lowered with its window and never clipped.
#[derive(Debug, Default)]
struct Gain {
    /// The running mean square of the windows that were not pauses; 0 until
    /// the first.
    level: f64,
    /// How many such windows were taken in, up to the time constant.
    taken: f64,
    /// The gain now, dB.
    db: f64,
}

impl Gain {
    fn wanted(&self) -> f64 {
        let level_dbfs = 10.0 * (self.level / (32_767.0 * 32_767.0)).log10();
        (GAIN_TARGET_DBFS - level_dbfs).clamp(GAIN_LEAST_DB, GAIN_MOST_DB)
    }

    /// Sets the level and the gain from the windows of the first block that
    /// are not pauses; once set, does nothing.
    fn start(&mut self, block: &[i16]) {
        if self.taken > 0.0 {
            return;
        }
        let pause = 32_767.0 * 10f64.powf(GAIN_PAUSE_DBFS / 20.0);
        let mut sum = 0.0;
        let mut count = 0.0;
        for window in block.chunks(LIMIT_WINDOW) {
            let power =
                window.iter().map(|&s| f64::from(s).powi(2)).sum::<f64>() / window.len() as f64;
            if power.sqrt() > pause {
                sum += power;
                count += 1.0;
            }
        }
        if count > 0.0 {
            self.level = sum / count;
            self.taken = count.min(GAIN_LEVEL_WINDOWS);
            self.db = self.wanted();
        }
    }

    /// The linear factor for a window whose RMS is `rms`.
    fn window(&mut self, rms: f64) -> f64 {
        let pause = 32_767.0 * 10f64.powf(GAIN_PAUSE_DBFS / 20.0);
        if self.taken > 0.0 && rms > pause {
            self.taken = (self.taken + 1.0).min(GAIN_LEVEL_WINDOWS);
            self.level += (rms * rms - self.level) / self.taken;
            let wanted = self.wanted();
            if wanted > self.db {
                self.db = (self.db + GAIN_UP_DB).min(wanted);
            } else {
                self.db = (self.db - GAIN_DOWN_DB).max(wanted);
            }
        }
        10f64.powf(self.db / 20.0)
    }
}

/// A stream's limiter: per 10 ms window (240 samples, one speaker chunk) the
/// gain that brings its RMS to the cap, taken at once when it must go down
/// and let up across the window when it can go back, then no sample past the
/// peak cap. Nothing is gathered ahead: each window is limited as it comes.
#[derive(Debug)]
struct Limiter {
    /// The gain the last window ended on.
    gain: f64,
    /// The RMS bound, dBFS.
    loudest_dbfs: f64,
    /// The peak bound, dBFS.
    peak_dbfs: f64,
    /// The long-form gain, lifted before the window is measured; none for a
    /// short sound or with the switch off.
    lift: Option<Gain>,
}

impl Default for Limiter {
    /// The short-sound bound.
    fn default() -> Self {
        Self {
            gain: 1.0,
            loudest_dbfs: crate::say::SOUND_LOUDEST_DBFS,
            peak_dbfs: crate::say::SOUND_PEAK_DBFS,
            lift: None,
        }
    }
}

impl Limiter {
    /// The long-form bound.
    fn long_form() -> Self {
        Self {
            gain: 1.0,
            loudest_dbfs: LONG_FORM_LOUDEST_DBFS,
            peak_dbfs: LONG_FORM_PEAK_DBFS,
            lift: None,
        }
    }

    /// The long-form bound with the gain: a lifted stream would be cut by the
    /// -20 RMS bound, so that is off ([`LIFTED_LOUDEST_DBFS`]) and the peak is
    /// [`LIFTED_PEAK_DBFS`] (the person's pick held it at -1).
    fn lifted() -> Self {
        Self {
            gain: 1.0,
            loudest_dbfs: LIFTED_LOUDEST_DBFS,
            peak_dbfs: LIFTED_PEAK_DBFS,
            lift: Some(Gain::default()),
        }
    }

    /// Limits `block` in place; how many of its 10 ms windows were lowered. A window shorter than 240 (the end of a block) is its own.
    fn limit(&mut self, block: &mut [i16]) -> u64 {
        let full = 32_767.0;
        let peak = (full * 10f64.powf(self.peak_dbfs / 20.0)).floor();
        // A little under the cap, so the rounding to whole samples never
        // puts a window a hair over it.
        let loudest = full * 10f64.powf(self.loudest_dbfs / 20.0) * RMS_MARGIN;
        let mut limited = 0;
        if let Some(lift) = self.lift.as_mut() {
            lift.start(block);
        }
        for window in block.chunks_mut(LIMIT_WINDOW) {
            let power = window.iter().map(|&s| f64::from(s).powi(2)).sum::<f64>();
            let raw_rms = (power / window.len() as f64).sqrt();
            // The gain's factor for this window; the window is measured as
            // it will play, so the bounds hold with the lift.
            let lift = self.lift.as_mut().map_or(1.0, |lift| lift.window(raw_rms));
            let rms = raw_rms * lift;
            // The loudest sample of the window brings the gain down too, so a
            // peak past the bound is lowered with the whole window (its shape
            // kept) and not cut flat. A part under 0.1% leaves the rounding
            // of whole samples no way to put one over.
            let top = window
                .iter()
                .map(|&s| f64::from(s).abs() * lift)
                .fold(0.0, f64::max);
            let by_rms = if rms > loudest { loudest / rms } else { 1.0 };
            let by_peak = if top > peak {
                peak * PEAK_MARGIN / top
            } else {
                1.0
            };
            let target = by_rms.min(by_peak);
            let start = if target < self.gain {
                target
            } else {
                self.gain
            };
            let count = window.len() as f64;
            // Lowered now, or still coming back up from an earlier window.
            let mut touched = target < 1.0 || self.gain < 1.0;
            for (at, sample) in window.iter_mut().enumerate() {
                // Down at once; back up across the window.
                let gain = if target < self.gain {
                    target
                } else {
                    start + (target - start) * (at as f64 + 1.0) / count
                };
                let mut value = f64::from(*sample) * lift * gain;
                // Only the rounding's own last hair: the gain above already
                // keeps every sample under the bound.
                if value.abs() > peak {
                    value = value.signum() * peak;
                    touched = true;
                }
                *sample = value.round() as i16;
            }
            self.gain = target;
            if touched {
                limited += 1;
            }
        }
        limited
    }
}

/// `speaker/audio`, read for as long as a stream is open.
struct Sound {
    blocks: Subscriber,
    counts: Counts,
    limiter: Limiter,
}

impl Sound {
    fn on(bus: &Bus) -> io::Result<Self> {
        Ok(Self {
            blocks: bus.subscribe_typed(Audio::SPEAKER_TOPIC, NAME, Audio::TYPE)?,
            counts: Counts::default(),
            limiter: if long_form_on() && gain_on() {
                Limiter::lifted()
            } else if long_form_on() {
                Limiter::long_form()
            } else {
                Limiter::default()
            },
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
                    let mut samples: Vec<i16> = bytes
                        .chunks_exact(2)
                        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
                        .collect();
                    // A computer's sound is held to the stream's bound
                    // (the skill sound's, or the long-form one with its
                    // switch); a live stream is lowered, and counted.
                    self.counts.limited += self.limiter.limit(&mut samples);
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

    /// A sine of `amplitude`, `count` samples.
    fn sine(amplitude: f64, count: usize) -> Vec<i16> {
        (0..count)
            .map(|at| (amplitude * (at as f64 * 0.21).sin()).round() as i16)
            .collect()
    }

    /// The loudest 10 ms (240 samples, aligned) of `samples` as RMS, and its peak.
    fn loudest(samples: &[i16]) -> (f64, f64) {
        let rms = samples
            .chunks(240)
            .map(|window| {
                let power: f64 = window.iter().map(|&s| f64::from(s).powi(2)).sum();
                (power / window.len() as f64).sqrt()
            })
            .fold(0.0, f64::max);
        let peak = samples
            .iter()
            .map(|&s| f64::from(s).abs())
            .fold(0.0, f64::max);
        (rms, peak)
    }

    #[test]
    fn a_quiet_sound_passes_the_limit_untouched() {
        let mut limiter = Limiter::default();
        let quiet = sine(1_000.0, 4_800);
        let mut block = quiet.clone();
        assert_eq!(limiter.limit(&mut block), 0);
        assert_eq!(block, quiet);
    }

    #[test]
    fn a_loud_sound_is_held_to_the_skill_sound_cap_window_by_window_and_counted() {
        let cap_rms = 32_767.0 * 10f64.powf(crate::say::SOUND_LOUDEST_DBFS / 20.0);
        let cap_peak = 32_767.0 * 10f64.powf(crate::say::SOUND_PEAK_DBFS / 20.0);
        // Whole windows, and blocks that end inside one.
        for size in [240, 2_400, 100, 777] {
            let mut limiter = Limiter::default();
            let mut played = Vec::new();
            let mut limited = 0;
            let loud = sine(20_000.0, 24_000);
            for part in loud.chunks(size) {
                let mut block = part.to_vec();
                limited += limiter.limit(&mut block);
                played.extend(block);
            }
            let (rms, peak) = loudest(&played);
            // Whole windows: never past the cap, rounding included. Blocks that
            // end inside a window count it from their own start: a loud tone
            // can pass a window counted from the stream's start by a little.
            let slack = if size % 240 == 0 { 1.0 } else { 1.01 };
            assert!(rms <= cap_rms * slack, "size {size}: {rms} past {cap_rms}");
            assert!(peak <= cap_peak, "size {size}: peak {peak} past {cap_peak}");
            assert!(limited >= 100, "size {size}: {limited} windows counted");
            assert_eq!(played.len(), loud.len());
        }
    }

    /// A sine of `rms_dbfs` RMS.
    fn tone_at(rms_dbfs: f64, count: usize) -> Vec<i16> {
        sine(
            32_767.0 * 10f64.powf(rms_dbfs / 20.0) * std::f64::consts::SQRT_2,
            count,
        )
    }

    #[test]
    fn a_stream_at_speech_level_passes_the_long_form_bound_untouched_and_the_short_one_lowers_it() {
        // About the lamp's own speech, -20 dBFS RMS; its peak is 3 dB over.
        let speech = tone_at(-20.5, 24_000);
        let mut long = Limiter::long_form();
        let mut played = speech.clone();
        assert_eq!(long.limit(&mut played), 0);
        assert_eq!(played, speech);
        let mut short = Limiter::default();
        let mut lowered = speech.clone();
        assert!(short.limit(&mut lowered) > 0);
        let (rms, _) = loudest(&lowered);
        assert!(rms < 32_767.0 * 10f64.powf(-29.5 / 20.0), "{rms}");
    }

    #[test]
    fn a_peak_past_the_bound_is_lowered_with_its_window_and_never_cut_flat() {
        // One 30,000 pulse a window on a quiet tone: quiet as RMS (about
        // -21 dBFS), 5 dB past the -6 dBFS peak bound. A hard clip would cut
        // the pulse and leave the tone as it was; the limiter scales the
        // whole window so the pulse fits and the tone keeps its shape.
        let cap_peak = 32_767.0 * 10f64.powf(LONG_FORM_PEAK_DBFS / 20.0);
        let mut input = sine(3_000.0, 4_800);
        for window in input.chunks_mut(240) {
            window[100] = 30_000;
        }
        let mut limiter = Limiter::long_form();
        let mut played = input.clone();
        let mut limited = 0;
        for part in played.chunks_mut(240) {
            limited += limiter.limit(part);
        }
        assert!(limited >= 19, "{limited}");
        let (_, peak) = loudest(&played);
        assert!(peak <= cap_peak, "{peak} past {cap_peak}");
        // Not flat: every window's pulse is the window's top, and the tone
        // beside it is scaled by the same gain (about 0.55), not untouched.
        for (before, after) in input.chunks(240).zip(played.chunks(240)).skip(1) {
            assert!(after[100] > 15_000, "pulse cut to {}", after[100]);
            let gain = f64::from(after[100]) / f64::from(before[100]);
            assert!((0.45..0.6).contains(&gain), "{gain}");
            for at in [20, 60, 140, 200] {
                if before[at].abs() > 1_500 {
                    let ratio = f64::from(after[at]) / f64::from(before[at]);
                    assert!(
                        (ratio - gain).abs() < 0.02,
                        "sample {at}: {ratio} vs {gain}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_long_form_stream_louder_than_speech_is_held_to_its_own_bound() {
        let cap_rms = 32_767.0 * 10f64.powf(LONG_FORM_LOUDEST_DBFS / 20.0);
        let cap_peak = 32_767.0 * 10f64.powf(LONG_FORM_PEAK_DBFS / 20.0);
        let mut limiter = Limiter::long_form();
        let mut played = Vec::new();
        let mut limited = 0;
        for part in sine(30_000.0, 24_000).chunks(240) {
            let mut block = part.to_vec();
            limited += limiter.limit(&mut block);
            played.extend(block);
        }
        let (rms, peak) = loudest(&played);
        assert!(rms <= cap_rms, "{rms} past {cap_rms}");
        assert!(peak <= cap_peak, "{peak} past {cap_peak}");
        assert!(limited >= 90, "{limited} windows counted");
        // Not lowered past its own bound: about -20, not -30.
        assert!(rms > 32_767.0 * 10f64.powf(-21.0 / 20.0), "{rms}");
    }

    #[test]
    fn a_short_skill_sound_is_still_held_at_minus_30_and_its_check_never_reads_the_switch() {
        // The skill sound's check is the sound op's, not the stream's: a clip
        // at speech level is refused, one under -30 passes. `decode_sound`
        // never reads the long-form switch, so this test does not touch it
        // (no test writes that process-wide static: the tests run in
        // parallel and a write races every test that opens a stream).
        fn wav(samples: &[i16]) -> Vec<u8> {
            let mut out = Vec::new();
            {
                let spec = hound::WavSpec {
                    channels: 1,
                    sample_rate: crate::say::SOUND_RATE,
                    bits_per_sample: 16,
                    sample_format: hound::SampleFormat::Int,
                };
                let mut writer =
                    hound::WavWriter::new(std::io::Cursor::new(&mut out), spec).unwrap();
                for &sample in samples {
                    writer.write_sample(sample).unwrap();
                }
                writer.finalize().unwrap();
            }
            out
        }
        let loud = crate::say::decode_sound(&wav(&tone_at(-20.5, 4_800)));
        let quiet = crate::say::decode_sound(&wav(&tone_at(-31.0, 4_800)));
        assert!(
            loud.is_err(),
            "a short sound at speech level must be refused"
        );
        assert!(quiet.is_ok());
    }

    /// Plays `input` through a lifted limiter in 240-sample blocks; the played
    /// sound and the gain (dB) each window ended on.
    fn lifted_through(input: &[i16]) -> (Vec<i16>, Vec<f64>) {
        let mut limiter = Limiter::lifted();
        let mut played = Vec::new();
        let mut gains = Vec::new();
        for part in input.chunks(240) {
            let mut block = part.to_vec();
            limiter.limit(&mut block);
            played.extend(block);
            gains.push(limiter.lift.as_ref().unwrap().db);
        }
        (played, gains)
    }

    #[test]
    fn a_quiet_long_form_stream_is_lifted_toward_minus_17_and_never_past_the_ceiling() {
        // Eight seconds at -26 dBFS: wants +9, held to the +6 ceiling.
        let (played, gains) = lifted_through(&tone_at(-26.0, 24_000 * 8));
        let top = gains.iter().copied().fold(0.0, f64::max);
        assert!((top - GAIN_MOST_DB).abs() < 0.01, "{top}");
        // A -21 stream wants +4: the last second is within 1 dB of -18.
        let (played_22, _) = lifted_through(&tone_at(-21.0, 24_000 * 8));
        let (rms, _) = loudest(&played_22[24_000 * 7..]);
        let dbfs = 20.0 * (rms / 32_767.0).log10();
        assert!((dbfs - GAIN_TARGET_DBFS).abs() < 1.0, "{dbfs}");
        assert!(played.len() == 24_000 * 8);
    }

    #[test]
    fn a_stream_at_the_target_gets_no_gain_and_a_louder_one_is_brought_down_to_it() {
        let (_, gains) = lifted_through(&tone_at(-16.5, 24_000 * 4));
        assert!(gains.iter().all(|&db| db.abs() < 0.6), "{gains:?}");
        // A -12 stream wants -5 and has it from the first block.
        let (played, gains) = lifted_through(&tone_at(-12.0, 24_000 * 4));
        assert!((gains[0] + 5.0).abs() < 0.5, "{}", gains[0]);
        assert!(gains.iter().all(|&db| db < 0.0), "{gains:?}");
        let (rms, _) = loudest(&played[24_000 * 3..]);
        let dbfs = 20.0 * (rms / 32_767.0).log10();
        assert!((dbfs - GAIN_TARGET_DBFS).abs() < 1.0, "{dbfs}");
    }

    /// The RMS of every 1 s stretch of `samples`, stepping by 100 ms, in dBFS.
    fn second_stretches(samples: &[i16]) -> Vec<f64> {
        samples
            .windows(24_000)
            .step_by(2_400)
            .map(|stretch| {
                let (rms, _) = loudest(stretch);
                20.0 * (rms / 32_767.0).log10()
            })
            .collect()
    }

    #[test]
    fn a_loud_stream_with_the_gain_on_ends_near_the_target_and_never_plays_at_its_own_level() {
        // Heron on #8399: -6 dBFS RMS must not play at -6 with the gain on.
        let (played, _) = lifted_through(&tone_at(-6.0, 24_000 * 6));
        let stretches = second_stretches(&played);
        let worst = stretches.iter().copied().fold(f64::MIN, f64::max);
        assert!(worst < GAIN_TARGET_DBFS + 1.0, "{worst}");
        let last = *stretches.last().unwrap();
        assert!((last - GAIN_TARGET_DBFS).abs() < 1.0, "{last}");
    }

    #[test]
    fn a_quiet_intro_gives_no_lift_and_the_speech_after_it_does_not_swell() {
        // Heron on #8399: 2 s of -40 (a pause: no gain, no seed), then -12.
        let mut input = tone_at(-40.0, 24_000 * 2);
        input.extend(tone_at(-12.0, 24_000 * 3));
        let (played, gains) = lifted_through(&input);
        assert!(
            gains[..199].iter().all(|&db| db == 0.0),
            "{:?}",
            &gains[..5]
        );
        let stretches = second_stretches(&played[24_000 * 2..]);
        let worst = stretches.iter().copied().fold(f64::MIN, f64::max);
        assert!(worst < GAIN_TARGET_DBFS + 3.0, "{worst}");
    }

    #[test]
    fn a_stream_that_turns_loud_while_the_gain_lags_is_held_at_the_window_bound() {
        // Quiet for 6 s (gain +6), then -8 dBFS: the gain comes down at 6 dB a
        // second, and no 10 ms window passes the lifted RMS bound meanwhile.
        let mut input = tone_at(-26.0, 24_000 * 6);
        input.extend(tone_at(-8.0, 24_000 * 3));
        let (played, _) = lifted_through(&input);
        let cap = 32_767.0 * 10f64.powf(LIFTED_LOUDEST_DBFS / 20.0);
        for window in played.chunks(240) {
            let (rms, _) = loudest(window);
            assert!(rms <= cap, "{rms} past {cap}");
        }
    }

    #[test]
    fn the_gain_is_there_from_the_first_block_and_moves_slowly_after() {
        let (_, gains) = lifted_through(&tone_at(-20.0, 24_000 * 4));
        // Wanted +3 from the first window on: no swell.
        assert!((gains[0] - 3.0).abs() < 0.5, "{}", gains[0]);
        // Then a louder second part brings it down at most 6 dB a second
        // (0.06 dB a window) and a quieter one up at most 3 dB a second.
        let mut input = tone_at(-20.0, 24_000 * 2);
        input.extend(tone_at(-26.0, 24_000 * 4));
        let (_, gains) = lifted_through(&input);
        for pair in gains.windows(2) {
            assert!(pair[1] - pair[0] <= GAIN_UP_DB + 1e-9, "{pair:?}");
            assert!(pair[0] - pair[1] <= GAIN_DOWN_DB + 1e-9, "{pair:?}");
        }
        assert!(gains.last().unwrap() > &4.0);
    }

    #[test]
    fn a_pause_does_not_lift_the_next_word() {
        let mut input = tone_at(-22.0, 24_000);
        input.extend(vec![0; 24_000 * 5]);
        input.extend(tone_at(-12.0, 4_800));
        let (_, gains) = lifted_through(&input);
        let before_pause = gains[99];
        let after_pause = gains[gains.len() - 21];
        assert!(
            (before_pause - after_pause).abs() < 1e-9,
            "{before_pause} {after_pause}"
        );
        // The loud word then brings it down, never up.
        assert!(gains[gains.len() - 1] <= after_pause);
    }

    #[test]
    fn a_lifted_stream_never_passes_minus_1_dbfs_and_a_peak_is_lowered_with_its_window() {
        let cap_peak = 32_767.0 * 10f64.powf(LIFTED_PEAK_DBFS / 20.0);
        let mut input = sine(3_000.0, 24_000 * 3);
        for window in input.chunks_mut(240) {
            window[100] = 20_000;
        }
        let (played, _) = lifted_through(&input);
        let (_, peak) = loudest(&played);
        assert!(peak <= cap_peak, "{peak} past {cap_peak}");
        // Not cut flat: the pulse stands well over the tone beside it.
        for window in played.chunks(240).skip(300) {
            assert!(window[100] > 10_000, "pulse cut to {}", window[100]);
        }
    }

    #[test]
    fn the_gain_switch_is_one_name_off_unless_set_and_the_short_bound_is_not_lifted() {
        assert_eq!(GAIN_ENVIRONMENT, "LAMP_LONG_FORM_GAIN");
        assert!(!gain_on());
        assert!(Limiter::default().lift.is_none());
        assert!(Limiter::long_form().lift.is_none());
    }

    #[test]
    fn the_long_form_switch_is_one_name_and_off_unless_set() {
        assert_eq!(LONG_FORM_ENVIRONMENT, "LAMP_STREAM_SPEECH_LEVEL");
        assert!(!long_form_on());
    }

    #[test]
    fn the_limit_lets_go_when_the_sound_is_quiet_again() {
        let mut limiter = Limiter::default();
        let mut loud = sine(20_000.0, 2_400);
        assert!(limiter.limit(&mut loud) > 0);
        // The gain comes back across one window; after it a quiet sound is
        // untouched and not counted.
        let mut quiet = sine(500.0, 2_400);
        let before = quiet.clone();
        let counted = limiter.limit(&mut quiet);
        assert!(counted <= 1, "{counted} windows counted while letting go");
        assert_eq!(quiet[240..], before[240..]);
    }

    #[test]
    fn an_open_stream_holds_a_loud_computer_to_the_cap_and_says_how_many_windows_it_lowered() {
        let _turn = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (bus, dir) = temp_bus("limited");
        let mut computers = bus
            .publisher_declared(Audio::SPEAKER_TOPIC, &Audio::declaration(), SOURCE)
            .unwrap();
        let (answer, _cancel, mut said, computer, served) = held(bus);
        let feed = Feed::new();
        answer.send(Answer::Begun(1)).unwrap();
        answer.send(Answer::Stream(Arc::clone(&feed))).unwrap();
        assert_eq!(line(&mut said)["state"], "open");
        // 120 ms of a loud tone, as 24 kHz S16LE blocks of 10 ms and 100 ms.
        let loud: Vec<i16> = sine(20_000.0, 2_880);
        let bytes: Vec<u8> = loud.iter().flat_map(|s| s.to_le_bytes()).collect();
        let header = Audio {
            sample_rate: 24_000,
            channels: 1,
            sample_format: Audio::S16LE.to_owned(),
            sequence: 1,
        };
        computers
            .publish_bytes(
                lamp_ipc::monotonic_us(),
                Audio::ENCODING,
                &header.block(&bytes).unwrap(),
            )
            .unwrap();
        let mut stats = serde_json::Value::Null;
        for _ in 0..40 {
            stats = line(&mut said);
            if stats["limited"].as_u64().is_some_and(|limited| limited > 0) {
                break;
            }
        }
        assert_eq!(stats["limited"], 12, "12 windows of 10 ms: {stats}");
        // What reached the feed is held to the cap.
        let (chunk, _) = feed.take().expect("the prefill is held");
        let (rms, peak) = loudest(&chunk);
        assert!(rms <= 32_767.0 * 0.0316 * 1.01, "{rms}");
        assert!(peak <= 8_232.0, "{peak}");
        answer
            .send(Answer::Done(Outcome::Dropped("done".into())))
            .unwrap();
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
    fn a_feed_says_what_it_holds_in_milliseconds_and_counts_the_speakers_gaps() {
        let feed = Feed::new();
        assert_eq!((feed.buffered_ms(), feed.underruns()), (0, 0));
        // 24 samples a millisecond at the speaker's rate.
        feed.push(&vec![1i16; 4_800]);
        assert_eq!(feed.buffered_ms(), 200);
        feed.take().expect("past the prefill");
        assert_eq!(feed.buffered_ms(), 190, "a chunk of 10 ms went");
        // The speaker reports its queue ran dry twice.
        feed.underrun();
        feed.underrun();
        assert_eq!(feed.underruns(), 2);
        assert_eq!(feed.buffered_ms(), 190, "a gap does not take sound");
    }

    #[test]
    fn an_open_streams_stats_carry_what_is_held_and_its_underruns() {
        let _turn = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (bus, dir) = temp_bus("buffered");
        let (answer, _cancel, mut said, computer, served) = held(bus);
        let feed = Feed::new();
        answer.send(Answer::Begun(1)).unwrap();
        answer.send(Answer::Stream(Arc::clone(&feed))).unwrap();
        assert_eq!(line(&mut said)["state"], "open");
        // 120 ms held (nothing taken: the speaker has not come for it) and the
        // speaker reports one gap: the next stats line says both.
        feed.push(&vec![1i16; 2_880]);
        feed.underrun();
        let mut stats = serde_json::Value::Null;
        for _ in 0..40 {
            stats = line(&mut said);
            if stats["underruns"] == 1 {
                break;
            }
        }
        assert_eq!(
            (stats["buffered_ms"].as_u64(), stats["underruns"].as_u64()),
            (Some(120), Some(1)),
            "{stats}"
        );
        answer
            .send(Answer::Done(Outcome::Dropped("done".into())))
            .unwrap();
        served.join().unwrap();
        drop(computer);
        std::fs::remove_dir_all(dir).unwrap();
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
