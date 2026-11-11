//! The speaker through the door: `speaker.stream` (the `speaker` grant,
//! docs/control.md, "The speaker").
//!
//! A paired computer sends sound for the lamp's speaker as binary WebSocket
//! messages; the door never opens the speaker. The voice owns it: lamp-control
//! asks the voice for a stream on the say socket ([`Voice`]), and then
//! publishes each block on the bus topic `speaker/audio`
//! (`lamp_msgs::Audio`, 24 kHz mono S16LE), which the voice reads only while
//! it holds that stream, so the echo canceller's reference, the person's
//! volume and the voice's own gates are the ones every sound meets.
//!
//! Off unless the owner switches it on (`--speaker on`). One stream on the
//! lamp, bound to the CONNECTION that opened it: a frame from another
//! connection of the same computer is `stray`, and a second `on` replaces the
//! first (`speaker.ended`, "replaced"). Never while the lamp is private or
//! muted, and never unmarked: the ring's mark is asked before the first block
//! leaves and renewed while it runs, and a block goes only once the ring said
//! it drew it (as the camera's and the microphone's do); blocks held back for
//! it count in `lost`, and no word from the ring within a second ends the
//! stream. The stream also ends when the voice says so (the lamp or the
//! person speaks, a hand on the head, "stop"), after 2 s with no frame, and
//! after 10 minutes.
//!
//! The frames come in on the server's connection task, which must never wait:
//! [`Speaker::frame`] takes a tiny lock of its own, never the door's or the
//! parts', and `try_send`s into the stream's bounded queue ([`QUEUED`] frames,
//! the newest dropped and counted in `lost`).
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::camera::{Drawn, MARK_EVERY, Mark, Private, Shown, shown};
use crate::parts::Events;
use crate::protocol::{Error, ErrorCode, SPEAKER_RATE_DEFAULT};

/// The speaker path's sample rate, the only one on `speaker/audio`.
pub const RATE: u32 = lamp_msgs::Audio::SPEAKER_SAMPLE_RATE;
/// The most bytes of samples one frame carries: 100 ms at 24 kHz is 4,800.
pub const FRAME_MAX_BYTES: usize = 9_600;
/// The most bytes a frame's JSON header takes (`{"ev":"speaker.audio","sequence":N}`
/// is about 40): a longer one is no frame of ours.
pub const HEADER_MAX_BYTES: usize = 256;
/// Frames waiting for the stream's thread, at most: past that the newest is
/// dropped, never waited for.
pub const QUEUED: usize = 20;
/// A stream with no frame this long ends, saying so.
pub const IDLE_AFTER: Duration = Duration::from_secs(2);
/// A stream ends after this long: it may start again.
pub const LIMIT: Duration = Duration::from_secs(10 * 60);
/// How often `speaker.stats` goes out while a stream runs.
pub const STATS_EVERY: Duration = Duration::from_secs(2);
/// How long the stream thread waits for a frame before it looks at the
/// gates, the voice and the clock again.
const LOOK_EVERY: Duration = Duration::from_millis(50);

/// Why a stream is refused on a lamp whose owner hasn't switched it on.
pub const SWITCHED_OFF: &str = "the speaker stream is off on this lamp (LAMP_CONTROL_SPEAKER)";
/// Why a stream is refused, or ends, with the lamp private.
pub const PRIVATE: &str = "the lamp is private";
/// Why a stream is refused, or ends, with the speaker muted (the person's
/// volume mute).
pub const MUTED: &str = "the lamp is muted";
/// Why a stream is refused where there is no voice to play it.
pub const NO_VOICE: &str = "this lamp's voice does not take a stream";
/// Why a stream ends with no word from the ring that it drew the mark.
pub const MARK_NOT_SHOWN: &str = "the ring is not showing the speaker's mark";
/// Why a stream ends when a second connection of the computer opened one.
pub const REPLACED: &str = "replaced";

/// What the voice tells lamp-control about an open stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Note {
    /// Its buffer and counts (each second or so).
    Stats {
        buffered_ms: u64,
        underruns: u64,
        limited: u64,
    },
    /// The voice ended it, with why (the lamp has words to say, the person
    /// talks, a hand on the head, "stop", privacy).
    Ended(String),
}

/// One open stream with the voice: dropped, it closes (the voice finishes
/// with its final chunk and fade).
pub trait Stream: Send {
    /// What the voice said since the last call, if anything.
    fn poll(&mut self) -> Option<Note>;
}

/// Asks the voice for a stream (the say socket's `stream` request): the
/// voice's refusal comes back in its words, [`NO_VOICE`] from one that does
/// not know the request.
pub trait Voice: Send + Sync {
    fn open(&self, by: &str) -> Result<Box<dyn Stream>, Error>;
}

/// Sends a block's payload (`lamp_msgs::Audio::block`) on `speaker/audio`.
pub type Publish = Arc<dyn Fn(&[u8]) -> io::Result<()> + Send + Sync>;

/// Whether the speaker is muted now (the person's volume mute).
pub type Muted = Arc<dyn Fn() -> bool + Send + Sync>;

/// A frame's samples at 24 kHz, from `bytes` at `rate`: 24 kHz as they are;
/// 16 kHz resampled 2:3 ([`Up`]).
pub struct Up {
    /// The last three source samples, for the cubic through the join of two
    /// frames.
    history: [f32; 3],
    /// Where the next output sample falls, in thirds of a source sample past
    /// the second of `history`; less than 3 (whole thirds, so no drift).
    phase: u32,
    started: bool,
}

impl Default for Up {
    fn default() -> Self {
        Self {
            history: [0.0; 3],
            phase: 0,
            started: false,
        }
    }
}

impl Up {
    /// `input` (16 kHz) as 24 kHz: three output samples for every two input,
    /// each a cubic (Catmull-Rom) through the four source samples around it, so
    /// a frame's join is as smooth as its inside.
    pub fn convert(&mut self, input: &[i16]) -> Vec<i16> {
        let mut out = Vec::with_capacity(input.len() * 3 / 2 + 3);
        for &sample in input {
            let next = f32::from(sample);
            if !self.started {
                // The first sample stands for the three before it.
                self.history = [next; 3];
                self.started = true;
            }
            // `history` holds p0, p1, p2 and `next` is p3: the interval
            // between p1 and p2 is [0, 1) of the phase.
            let [p0, p1, p2] = self.history;
            let p3 = next;
            while self.phase < 3 {
                let t = self.phase as f32 / 3.0;
                let value = 0.5
                    * (2.0 * p1
                        + (-p0 + p2) * t
                        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t * t
                        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t * t * t);
                out.push(value.round().clamp(-32768.0, 32767.0) as i16);
                self.phase += 2;
            }
            self.phase -= 3;
            self.history = [p1, p2, p3];
        }
        out
    }
}

/// The counts a stream keeps, read by `speaker.stats`.
#[derive(Default)]
struct Counts {
    /// Blocks lost: gaps in what was sent, the ones the queue had no room for
    /// and the ones held back for the ring's mark.
    lost: AtomicU64,
    /// Binary messages from the computer's other connections, or too big, or
    /// not whole samples, while this stream runs.
    stray: AtomicU64,
}

/// What `frame` finds: the open stream's way in.
struct Slot {
    conn: u64,
    client: String,
    turn: u64,
    queue: SyncSender<Vec<u8>>,
    counts: Arc<Counts>,
}

/// The open stream, if any: one on the lamp. A tiny lock of its own.
#[derive(Default)]
struct Open {
    slot: Mutex<Option<Slot>>,
    turns: AtomicU64,
}

/// What stops the speaker, asked before a stream starts and in each look.
#[derive(Clone)]
struct Gates {
    private: Private,
    muted: Muted,
}

impl Gates {
    fn closed(&self) -> Option<(ErrorCode, &'static str)> {
        if (self.private)() {
            Some((ErrorCode::Private, PRIVATE))
        } else if (self.muted)() {
            Some((ErrorCode::Refused, MUTED))
        } else {
            None
        }
    }
}

/// The speaker as the door has it.
pub struct Speaker {
    voice: Option<Arc<dyn Voice>>,
    publish: Option<Publish>,
    mark: Option<(Mark, Drawn)>,
    gates: Gates,
    /// The owner's switch (`--speaker on`); off, every stream is refused.
    switched: bool,
    open: Arc<Open>,
    idle_after: Duration,
    stats_every: Duration,
}

impl Speaker {
    pub fn new(private: Private, muted: Muted) -> Self {
        Self {
            voice: None,
            publish: None,
            mark: None,
            gates: Gates { private, muted },
            switched: false,
            open: Arc::default(),
            idle_after: IDLE_AFTER,
            stats_every: STATS_EVERY,
        }
    }

    /// The voice to ask, and where the blocks go.
    pub fn playing(mut self, voice: Arc<dyn Voice>, publish: Publish) -> Self {
        self.voice = Some(voice);
        self.publish = Some(publish);
        self
    }

    /// The ring's mark while it plays, and its word that it drew it: a block
    /// goes only once that word has come; none, no speaker at all.
    pub fn marking(mut self, mark: Mark, drawn: Drawn) -> Self {
        self.mark = Some((mark, drawn));
        self
    }

    /// The owner's switch: off unless set.
    pub fn switched(mut self, on: bool) -> Self {
        self.switched = on;
        self
    }

    /// Shorter waits, for tests.
    pub fn timing(mut self, idle_after: Duration, stats_every: Duration) -> Self {
        self.idle_after = idle_after;
        self.stats_every = stats_every;
        self
    }

    /// `speaker.stream`, on or off, from connection `conn` of `client`. On
    /// again, from any connection of any computer, replaces the stream that
    /// runs, whose computer is told `speaker.ended`, "replaced".
    pub fn stream(
        &self,
        conn: u64,
        client: &str,
        name: &str,
        on: bool,
        rate: Option<u32>,
        events: Events,
    ) -> Result<Value, Error> {
        if !on {
            self.end_of(conn, client);
            return Ok(json!({"stream": "off"}));
        }
        if !self.switched {
            return Err(Error::new(ErrorCode::Refused, SWITCHED_OFF));
        }
        if let Some((code, why)) = self.gates.closed() {
            return Err(Error::new(code, why));
        }
        let (Some(voice), Some(publish), Some(mark)) =
            (self.voice.clone(), self.publish.clone(), self.mark.clone())
        else {
            return Err(Error::new(ErrorCode::Refused, NO_VOICE));
        };
        let rate = rate.unwrap_or(SPEAKER_RATE_DEFAULT);
        // The voice is asked before the old stream goes: a refusal leaves it be.
        let stream = voice.open(name)?;
        let turn = self.open.turns.fetch_add(1, Ordering::Relaxed) + 1;
        let (queue, frames) = mpsc::sync_channel::<Vec<u8>>(QUEUED);
        let counts = Arc::new(Counts::default());
        let old = {
            let mut slot = self.open.slot.lock().expect("slot");
            slot.replace(Slot {
                conn,
                client: client.to_owned(),
                turn,
                queue,
                counts: counts.clone(),
            })
        };
        if let Some(old) = old {
            // Its thread sees its slot gone and ends, telling why.
            events(&old.client, ended_event(REPLACED, old.turn));
        }
        let run = Run {
            open: self.open.clone(),
            gates: self.gates.clone(),
            events,
            publish,
            mark: mark.0,
            drawn: mark.1,
            client: client.to_owned(),
            turn,
            rate,
            counts,
            idle_after: self.idle_after,
            stats_every: self.stats_every,
        };
        std::thread::spawn(move || run.go(frames, stream));
        Ok(json!({
            "stream": "on", "sample_rate": rate, "channels": 1, "format": "s16le",
            "frame_ms": [10, 100], "buffer_ms": 300, "source": "speaker", "turn": turn,
        }))
    }

    /// The samples of a frame as a computer sends it (docs/control.md, "The
    /// speaker"; the microphone's and the camera's frames the other way): the
    /// header's length (4 bytes, big-endian), the JSON header (`ev`
    /// `speaker.audio`, `sequence` a number), then the samples. None when the
    /// length is 0, over [`HEADER_MAX_BYTES`] or past the message, or the
    /// header is not that object: no frame of ours, so no sound.
    fn samples_of(data: &[u8]) -> Option<&[u8]> {
        let length = u32::from_be_bytes(data.get(..4)?.try_into().ok()?) as usize;
        if length == 0 || length > HEADER_MAX_BYTES {
            return None;
        }
        let header: Value = serde_json::from_slice(data.get(4..4 + length)?).ok()?;
        if header.get("ev")? != "speaker.audio" || !header.get("sequence")?.is_u64() {
            return None;
        }
        data.get(4 + length..)
    }

    /// A binary message from connection `conn`, a client known: the header's
    /// length, the JSON header, then the samples. Dropped, with no answer,
    /// unless that connection's own stream runs; never waits.
    /// Whether it was that stream's own (queued, or dropped for want of room).
    pub fn frame(&self, conn: u64, data: &[u8]) -> bool {
        let slot = self.open.slot.lock().expect("slot");
        let Some(slot) = slot.as_ref() else {
            return false;
        };
        if slot.conn != conn {
            slot.counts.stray.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        // A message that is no frame (no header of ours) or whose samples are
        // of no size a stream takes (empty, odd, over the most) is no sound:
        // counted stray, never queued, and it does not keep the stream's idle
        // clock or the door's own from running out.
        let Some(samples) = Self::samples_of(data) else {
            slot.counts.stray.fetch_add(1, Ordering::Relaxed);
            return false;
        };
        if samples.is_empty() || samples.len() > FRAME_MAX_BYTES || !samples.len().is_multiple_of(2)
        {
            slot.counts.stray.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        match slot.queue.try_send(samples.to_vec()) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                slot.counts.lost.fetch_add(1, Ordering::Relaxed);
            }
            Err(TrySendError::Disconnected(_)) => return false,
        }
        true
    }

    /// Ends `conn`'s stream, if it runs, without an event: its own `off`,
    /// `stop`, the connection closing, or a re-pairing without the grant.
    pub fn end_of(&self, conn: u64, client: &str) {
        let mut slot = self.open.slot.lock().expect("slot");
        if slot
            .as_ref()
            .is_some_and(|slot| slot.conn == conn && slot.client == client)
        {
            *slot = None;
        }
    }

    /// Ends the stream bound to connection `conn`, if it runs, without an
    /// event: that connection closed.
    pub fn end_conn(&self, conn: u64) {
        let mut slot = self.open.slot.lock().expect("slot");
        if slot.as_ref().is_some_and(|slot| slot.conn == conn) {
            *slot = None;
        }
    }

    /// Ends `client`'s stream from whichever connection, without an event
    /// (a revoke, or a re-pairing without the grant).
    pub fn end_client(&self, client: &str) {
        let mut slot = self.open.slot.lock().expect("slot");
        if slot.as_ref().is_some_and(|slot| slot.client == client) {
            *slot = None;
        }
    }

    /// Whether `client`'s stream runs now.
    pub fn streaming(&self, client: &str) -> bool {
        self.open
            .slot
            .lock()
            .expect("slot")
            .as_ref()
            .is_some_and(|slot| slot.client == client)
    }
}

fn ended_event(why: &str, turn: u64) -> Value {
    json!({"ev": "speaker.ended", "why": why, "turn": turn})
}

/// One stream's thread.
struct Run {
    open: Arc<Open>,
    gates: Gates,
    events: Events,
    publish: Publish,
    mark: Mark,
    drawn: Drawn,
    client: String,
    turn: u64,
    rate: u32,
    counts: Arc<Counts>,
    idle_after: Duration,
    stats_every: Duration,
}

impl Run {
    /// Whether this thread's stream still holds the slot.
    fn mine(&self) -> bool {
        self.open
            .slot
            .lock()
            .expect("slot")
            .as_ref()
            .is_some_and(|slot| slot.turn == self.turn)
    }

    fn go(self, frames: mpsc::Receiver<Vec<u8>>, mut voice: Box<dyn Stream>) {
        let started = Instant::now();
        let mut last_frame = started;
        let mut last_mark: Option<Instant> = None;
        let mut last_stats = started;
        let mut sequence = 0u64;
        let mut lost_seen = 0u64;
        let mut up = Up::default();
        let mut stats = (0u64, 0u64, 0u64);
        let why = loop {
            if !self.mine() {
                // Replaced or ended by its own off: the replaced computer was
                // told, the other need not be.
                self.unmark();
                return;
            }
            if let Some((_, why)) = self.gates.closed() {
                break why.to_owned();
            }
            if started.elapsed() >= LIMIT {
                break "limit".to_owned();
            }
            if last_mark.is_none_or(|at| at.elapsed() >= MARK_EVERY) {
                (self.mark)(&self.client, false);
                last_mark = Some(Instant::now());
            }
            let marked = shown(&self.drawn, &self.client, started);
            if marked == Shown::Never {
                break MARK_NOT_SHOWN.to_owned();
            }
            while let Some(note) = voice.poll() {
                match note {
                    Note::Stats {
                        buffered_ms,
                        underruns,
                        limited,
                    } => stats = (buffered_ms, underruns, limited),
                    Note::Ended(why) => {
                        self.finish(&why);
                        return;
                    }
                }
            }
            match frames.recv_timeout(LOOK_EVERY) {
                Ok(bytes) => {
                    if bytes.is_empty()
                        || bytes.len() > FRAME_MAX_BYTES
                        || !bytes.len().is_multiple_of(2)
                    {
                        self.counts.stray.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                    // Only a frame of sound keeps the stream from going idle.
                    last_frame = Instant::now();
                    let samples: Vec<i16> = bytes
                        .chunks_exact(2)
                        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
                        .collect();
                    let samples = if self.rate == RATE {
                        samples
                    } else {
                        up.convert(&samples)
                    };
                    // What the queue dropped counts in the sequence, so the
                    // voice sees the gap.
                    let lost = self.counts.lost.load(Ordering::Relaxed);
                    sequence += lost - lost_seen;
                    lost_seen = lost;
                    if marked != Shown::Yes {
                        // Held back for the ring's word: counted lost.
                        self.counts.lost.fetch_add(1, Ordering::Relaxed);
                        lost_seen += 1;
                        sequence += 1;
                        continue;
                    }
                    let header = lamp_msgs::Audio {
                        sample_rate: RATE,
                        channels: 1,
                        sample_format: lamp_msgs::Audio::S16LE.into(),
                        sequence,
                    };
                    sequence += 1;
                    let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
                    let Ok(payload) = header.block(&bytes) else {
                        continue;
                    };
                    if (self.publish)(&payload).is_err() {
                        break "the lamp's speaker is not reachable".to_owned();
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    self.unmark();
                    return;
                }
            }
            if last_frame.elapsed() >= self.idle_after {
                break "idle".to_owned();
            }
            if last_stats.elapsed() >= self.stats_every {
                last_stats = Instant::now();
                (self.events)(
                    &self.client,
                    json!({
                        "ev": "speaker.stats", "turn": self.turn,
                        "buffered_ms": stats.0,
                        "lost": self.counts.lost.load(Ordering::Relaxed),
                        "underruns": stats.1, "limited": stats.2,
                        "stray": self.counts.stray.load(Ordering::Relaxed),
                    }),
                );
            }
        };
        self.finish(&why);
    }

    /// The stream ended by itself: its slot goes, the mark is let lapse and the
    /// computer is told why.
    fn finish(&self, why: &str) {
        let ours = {
            let mut slot = self.open.slot.lock().expect("slot");
            let ours = slot.as_ref().is_some_and(|slot| slot.turn == self.turn);
            if ours {
                *slot = None;
            }
            ours
        };
        self.unmark();
        // A stream replaced or ended by its own `off` meanwhile was told, or
        // is not to be, by whoever ended it.
        if ours {
            (self.events)(&self.client, ended_event(why, self.turn));
        }
    }

    /// The mark is not asked again: it lapses within its 2 s hold.
    fn unmark(&self) {}
}

/// How long a stream's block may wait on the bus before it is stale: a
/// late block is skipped, never played behind the rest.
const BLOCK_LIFETIME: Duration = Duration::from_millis(250);

/// `speaker/audio` on the bus, published as `control`.
pub fn on_bus(bus: &lamp_bus::Bus) -> io::Result<Publish> {
    let mut publisher = bus.publisher_declared(
        lamp_msgs::Audio::SPEAKER_TOPIC,
        &lamp_msgs::Audio::speaker_declaration().lasting(BLOCK_LIFETIME),
        "control",
    )?;
    publisher.expires_after(Some(BLOCK_LIFETIME));
    let publisher = Mutex::new(publisher);
    Ok(Arc::new(move |payload| {
        publisher
            .lock()
            .map_err(|_| io::Error::other("the speaker publisher is poisoned"))?
            .publish_bytes(
                lamp_ipc::monotonic_us(),
                lamp_msgs::Audio::ENCODING,
                payload,
            )
            .map(|_| ())
    }))
}

/// The voice's say socket, asked for a stream (`{"skill":"control",
/// "stream":{"by":CLIENT},"expires_in_ms":N}`): it answers one line, either
/// `{"event":"stream","state":"open","buffer_ms":N}` or the usual refusal
/// (`{"outcome":"refused","why":...}`), and then, while the stream is open,
/// `{"event":"stats",...}` lines and at the end `{"outcome":...}`. Closing the
/// connection ends the stream on the voice's side.
pub struct SayVoice {
    pub path: std::path::PathBuf,
}

/// How long the voice may take to answer the request.
const OPEN_WITHIN: Duration = Duration::from_secs(2);

impl Voice for SayVoice {
    fn open(&self, by: &str) -> Result<Box<dyn Stream>, Error> {
        use std::io::{Read, Write};
        let not_taken = |error: io::Error| {
            Error::new(
                ErrorCode::Refused,
                format!("the voice did not take it: {error}"),
            )
        };
        let mut socket = std::os::unix::net::UnixStream::connect(&self.path).map_err(not_taken)?;
        let line = json!({
            "skill": "control", "stream": {"by": by},
            "expires_in_ms": crate::parts::SAY_EXPIRES_MS,
        });
        writeln!(socket, "{line}").map_err(not_taken)?;
        socket
            .set_read_timeout(Some(OPEN_WITHIN))
            .map_err(not_taken)?;
        // The first line, a byte at a time so nothing after it is lost.
        let mut first = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            match socket.read(&mut byte) {
                Ok(0) => {
                    return Err(Error::new(ErrorCode::Refused, "the voice gave no answer"));
                }
                Ok(_) if byte[0] == b'\n' => break,
                Ok(_) if first.len() < 4096 => first.push(byte[0]),
                Ok(_) => {
                    return Err(Error::new(
                        ErrorCode::Refused,
                        "the voice's answer is too long",
                    ));
                }
                Err(error) => return Err(not_taken(error)),
            }
        }
        let said: Value = serde_json::from_slice(&first)
            .map_err(|_| Error::new(ErrorCode::Refused, "the voice's answer is not understood"))?;
        if said["event"] != "stream" || said["state"] != "open" {
            let why = said["why"].as_str().unwrap_or("the voice refused it");
            // A voice from before the request knows it as no say request.
            let why = if why == crate::parts::NOT_ITS_PROTOCOL {
                NO_VOICE
            } else {
                why
            };
            return Err(Error::new(ErrorCode::Refused, why));
        }
        socket.set_nonblocking(true).map_err(not_taken)?;
        Ok(Box::new(SaySocket {
            socket,
            held: Vec::new(),
            done: false,
        }))
    }
}

/// One open stream with the voice, read without waiting.
struct SaySocket {
    socket: std::os::unix::net::UnixStream,
    /// Bytes read that are not yet a whole line.
    held: Vec<u8>,
    /// The voice's end was told.
    done: bool,
}

impl Stream for SaySocket {
    fn poll(&mut self) -> Option<Note> {
        use std::io::Read;
        if self.done {
            return None;
        }
        let mut chunk = [0u8; 1024];
        loop {
            match self.socket.read(&mut chunk) {
                Ok(0) => {
                    self.done = true;
                    if let Some(note) = self.line() {
                        return Some(note);
                    }
                    return Some(Note::Ended("the voice went away".to_owned()));
                }
                Ok(n) => self.held.extend_from_slice(&chunk[..n]),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    self.done = true;
                    return Some(Note::Ended("the voice went away".to_owned()));
                }
            }
            if self.held.len() > 64 * 1024 {
                self.done = true;
                return Some(Note::Ended("the voice's lines are too long".to_owned()));
            }
        }
        self.line()
    }
}

impl SaySocket {
    /// The next whole line the voice said, as a note; lines it says that are
    /// of no use here are skipped.
    fn line(&mut self) -> Option<Note> {
        while let Some(end) = self.held.iter().position(|&byte| byte == b'\n') {
            let line: Vec<u8> = self.held.drain(..=end).collect();
            let Ok(said) = serde_json::from_slice::<Value>(&line) else {
                continue;
            };
            if said["event"] == "stats" {
                let number = |key: &str| said[key].as_u64().unwrap_or(0);
                return Some(Note::Stats {
                    buffered_ms: number("buffered_ms"),
                    underruns: number("underruns"),
                    limited: number("limited"),
                });
            }
            if let Some(word) = said["outcome"].as_str() {
                self.done = true;
                let why = match said["why"].as_str() {
                    Some(why) => why.to_owned(),
                    None if word == "spoken" => "finished".to_owned(),
                    None => word.to_owned(),
                };
                return Some(Note::Ended(why));
            }
        }
        None
    }
}

/// The gates a speaker reads from the lamp's watch: whether the lamp is
/// private, and whether the person's volume mute is on. Without a watch the
/// lamp is taken as private, so nothing plays.
pub fn gates(watch: Option<Arc<Mutex<crate::watch::Watch>>>) -> (Private, Muted) {
    let private_watch = watch.clone();
    let private: Private = Arc::new(move || {
        private_watch
            .as_ref()
            .is_none_or(|watch| watch.lock().map_or(true, |watch| watch.private()))
    });
    let muted: Muted = Arc::new(move || {
        watch
            .as_ref()
            .is_some_and(|watch| watch.lock().is_ok_and(|watch| watch.muted()))
    });
    (private, muted)
}

/// The speaker, given to the lamp's parts and to the server alike, once the
/// lamp has built it.
pub type Handle = Arc<std::sync::OnceLock<Arc<Speaker>>>;

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::AtomicBool;

    use super::*;

    /// What the fake voice says next, and whether it was closed.
    #[derive(Default)]
    struct Said {
        notes: Mutex<VecDeque<Note>>,
        closed: AtomicBool,
        asked: Mutex<Vec<String>>,
        refuse: Mutex<Option<Error>>,
    }

    struct FakeStream(Arc<Said>);

    impl Stream for FakeStream {
        fn poll(&mut self) -> Option<Note> {
            self.0.notes.lock().unwrap().pop_front()
        }
    }

    impl Drop for FakeStream {
        fn drop(&mut self) {
            self.0.closed.store(true, Ordering::SeqCst);
        }
    }

    struct FakeVoice(Arc<Said>);

    impl Voice for FakeVoice {
        fn open(&self, by: &str) -> Result<Box<dyn Stream>, Error> {
            self.0.asked.lock().unwrap().push(by.to_owned());
            if let Some(error) = self.0.refuse.lock().unwrap().take() {
                return Err(error);
            }
            Ok(Box::new(FakeStream(self.0.clone())))
        }
    }

    struct Rig {
        speaker: Speaker,
        said: Arc<Said>,
        blocks: Arc<Mutex<Vec<Vec<u8>>>>,
        told: Arc<Mutex<Vec<(String, Value)>>>,
        private: Arc<AtomicBool>,
        muted: Arc<AtomicBool>,
        drawn: Arc<AtomicBool>,
    }

    fn rig() -> Rig {
        let private = Arc::new(AtomicBool::new(false));
        let muted = Arc::new(AtomicBool::new(false));
        let drawn = Arc::new(AtomicBool::new(true));
        let said = Arc::new(Said::default());
        let blocks = Arc::new(Mutex::new(Vec::new()));
        let (b, d) = (blocks.clone(), drawn.clone());
        let publish: Publish = Arc::new(move |payload| {
            b.lock().unwrap().push(payload.to_vec());
            Ok(())
        });
        let marks: Mark = Arc::new(|_, _| {});
        let draw: Drawn = Arc::new(move |_| d.load(Ordering::SeqCst).then(Instant::now));
        let (p, m) = (private.clone(), muted.clone());
        let speaker = Speaker::new(
            Arc::new(move || p.load(Ordering::SeqCst)),
            Arc::new(move || m.load(Ordering::SeqCst)),
        )
        .playing(Arc::new(FakeVoice(said.clone())), publish)
        .marking(marks, draw)
        .switched(true)
        .timing(Duration::from_millis(400), Duration::from_millis(100));
        Rig {
            speaker,
            said,
            blocks,
            told: Arc::default(),
            private,
            muted,
            drawn,
        }
    }

    impl Rig {
        fn events(&self) -> Events {
            let told = self.told.clone();
            Arc::new(move |client, event| told.lock().unwrap().push((client.to_owned(), event)))
        }

        fn on(&self, conn: u64, client: &str, rate: Option<u32>) -> Result<Value, Error> {
            self.speaker
                .stream(conn, client, "a computer", true, rate, self.events())
        }

        fn told_ev(&self, ev: &str) -> Vec<Value> {
            self.told
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, event)| event["ev"] == ev)
                .map(|(_, event)| event.clone())
                .collect()
        }
    }

    fn pcm(samples: usize) -> Vec<u8> {
        (0..samples)
            .flat_map(|n| ((n as i16) * 7).to_le_bytes())
            .collect()
    }

    /// A frame as a computer sends it (docs/control.md, "The speaker"): the
    /// header's length (4 bytes, big-endian), the JSON header, the samples.
    fn framed(samples: &[u8]) -> Vec<u8> {
        framed_with(br#"{"ev":"speaker.audio","sequence":3}"#, samples)
    }

    fn framed_with(header: &[u8], samples: &[u8]) -> Vec<u8> {
        let mut frame = (header.len() as u32).to_be_bytes().to_vec();
        frame.extend_from_slice(header);
        frame.extend_from_slice(samples);
        frame
    }

    fn wait(mut ready: impl FnMut() -> bool) -> bool {
        for _ in 0..100 {
            if ready() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    /// A fake say socket: answers each request with `lines`, one a line, then
    /// holds the connection open for `hold`.
    fn say_socket(
        name: &str,
        lines: Vec<String>,
        hold: Duration,
    ) -> (std::path::PathBuf, std::thread::JoinHandle<String>) {
        use std::io::{BufRead, Write};
        let path =
            std::env::temp_dir().join(format!("lamp-control-say-{name}-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let join = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = String::new();
            std::io::BufReader::new(socket.try_clone().unwrap())
                .read_line(&mut request)
                .unwrap();
            for line in lines {
                writeln!(socket, "{line}").unwrap();
                std::thread::sleep(Duration::from_millis(20));
            }
            std::thread::sleep(hold);
            request
        });
        (path, join)
    }

    #[test]
    fn the_say_socket_opens_a_stream_and_tells_its_stats_and_its_end() {
        let (path, join) = say_socket(
            "open",
            vec![
                r#"{"event":"stream","state":"open","buffer_ms":300}"#.to_owned(),
                r#"{"event":"stats","buffered_ms":120,"underruns":1,"limited":2}"#.to_owned(),
                r#"{"outcome":"dropped","why":"the lamp has something to say"}"#.to_owned(),
            ],
            Duration::from_millis(200),
        );
        let mut stream = SayVoice { path: path.clone() }.open("c_1").unwrap();
        let mut notes = Vec::new();
        for _ in 0..100 {
            if let Some(note) = stream.poll() {
                notes.push(note);
            }
            if notes.len() == 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(
            notes,
            vec![
                Note::Stats {
                    buffered_ms: 120,
                    underruns: 1,
                    limited: 2
                },
                Note::Ended("the lamp has something to say".to_owned()),
            ]
        );
        let request: Value = serde_json::from_str(&join.join().unwrap()).unwrap();
        assert_eq!(request["skill"], "control");
        assert_eq!(request["stream"]["by"], "c_1");
        assert_eq!(request["expires_in_ms"], 30_000);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_voices_refusal_is_said_in_its_words_and_an_older_voice_does_not_take_a_stream() {
        let (path, join) = say_socket(
            "refuse",
            vec![r#"{"outcome":"refused","why":"the lamp is speaking"}"#.to_owned()],
            Duration::ZERO,
        );
        let error = SayVoice { path: path.clone() }
            .open("c_1")
            .err()
            .expect("refused");
        assert_eq!(error.why, "the lamp is speaking");
        join.join().unwrap();
        let (path2, join) = say_socket(
            "old",
            vec![r#"{"outcome":"refused","why":"not a say request"}"#.to_owned()],
            Duration::ZERO,
        );
        let error = SayVoice {
            path: path2.clone(),
        }
        .open("c_1")
        .err()
        .expect("refused");
        assert_eq!(error.why, NO_VOICE);
        join.join().unwrap();
        // No voice at all: the connection itself is refused, in words.
        let error = SayVoice {
            path: std::path::PathBuf::from("/nonexistent/say.sock"),
        }
        .open("c_1")
        .err()
        .expect("refused");
        assert!(
            error.why.starts_with("the voice did not take it"),
            "{}",
            error.why
        );
        let _ = (std::fs::remove_file(path), std::fs::remove_file(path2));
    }

    #[test]
    fn off_unless_the_owner_switches_it_on() {
        let rig = rig();
        let off = Speaker::new(Arc::new(|| false), Arc::new(|| false));
        let error = off
            .stream(1, "c_1", "a", true, None, rig.events())
            .unwrap_err();
        assert_eq!(error.why, SWITCHED_OFF);
        assert!(rig.said.asked.lock().unwrap().is_empty());
    }

    #[test]
    fn a_stream_is_refused_while_the_lamp_is_private_or_muted() {
        let rig = rig();
        rig.private.store(true, Ordering::SeqCst);
        let error = rig.on(1, "c_1", None).unwrap_err();
        assert_eq!(
            (error.code, error.why.as_str()),
            (ErrorCode::Private, PRIVATE)
        );
        rig.private.store(false, Ordering::SeqCst);
        rig.muted.store(true, Ordering::SeqCst);
        let error = rig.on(1, "c_1", None).unwrap_err();
        assert_eq!(error.why, MUTED);
        assert!(
            rig.said.asked.lock().unwrap().is_empty(),
            "the voice is not asked"
        );
    }

    #[test]
    fn the_voices_refusal_comes_back_in_its_words_and_leaves_the_running_stream() {
        let rig = rig();
        rig.on(1, "c_1", None).unwrap();
        *rig.said.refuse.lock().unwrap() =
            Some(Error::new(ErrorCode::Busy, "the lamp is speaking"));
        let error = rig.on(2, "c_2", None).unwrap_err();
        assert_eq!(error.why, "the lamp is speaking");
        assert!(
            rig.speaker.streaming("c_1"),
            "the stream that runs is left be"
        );
    }

    #[test]
    fn a_24_khz_frame_goes_as_one_block_marked_and_numbered() {
        let rig = rig();
        let answer = rig.on(1, "c_1", Some(24_000)).unwrap();
        assert_eq!(answer["sample_rate"], 24_000);
        rig.speaker.frame(1, &framed(&pcm(480)));
        rig.speaker.frame(1, &framed(&pcm(480)));
        assert!(wait(|| rig.blocks.lock().unwrap().len() == 2));
        let blocks = rig.blocks.lock().unwrap();
        let (first, samples) = lamp_msgs::Audio::split(&blocks[0]).unwrap();
        assert_eq!(
            (first.sample_rate, first.channels, first.sequence),
            (24_000, 1, 0)
        );
        assert_eq!(samples, &pcm(480)[..], "24 kHz goes as it came");
        assert_eq!(lamp_msgs::Audio::split(&blocks[1]).unwrap().0.sequence, 1);
    }

    #[test]
    fn a_16_khz_frame_is_resampled_to_24_khz_three_for_two() {
        let rig = rig();
        rig.on(1, "c_1", None).unwrap();
        rig.speaker.frame(1, &framed(&pcm(160)));
        rig.speaker.frame(1, &framed(&pcm(160)));
        assert!(wait(|| rig.blocks.lock().unwrap().len() == 2));
        let blocks = rig.blocks.lock().unwrap();
        let sizes: Vec<usize> = blocks
            .iter()
            .map(|block| lamp_msgs::Audio::split(block).unwrap().1.len() / 2)
            .collect();
        assert_eq!(
            sizes.iter().sum::<usize>(),
            480,
            "{sizes:?}: 320 in, 480 out"
        );
    }

    #[test]
    fn the_resampler_keeps_a_steady_tone_steady_across_frames() {
        let tone = |n: usize| (f32::from(1000u16) * (n as f32 * 0.05).sin()) as i16;
        let source: Vec<i16> = (0..320).map(tone).collect();
        let mut whole = Up::default();
        let all = whole.convert(&source);
        let mut split = Up::default();
        let mut parts = split.convert(&source[..100]);
        parts.extend(split.convert(&source[100..]));
        assert_eq!(all, parts, "frame boundaries make no difference");
        // A constant stays a constant.
        let mut up = Up::default();
        assert!(up.convert(&[1234; 64]).iter().all(|&s| s == 1234));
    }

    #[test]
    fn a_frame_from_another_connection_of_the_computer_is_stray() {
        let rig = rig();
        rig.on(1, "c_1", Some(24_000)).unwrap();
        rig.speaker.frame(2, &framed(&pcm(480)));
        rig.speaker.frame(1, &framed(&pcm(480)));
        assert!(wait(|| rig.blocks.lock().unwrap().len() == 1));
        assert!(wait(|| {
            rig.told_ev("speaker.stats")
                .last()
                .is_some_and(|stats| stats["stray"] == 1)
        }));
    }

    #[test]
    fn frames_of_no_size_a_stream_takes_are_stray_and_never_keep_it_alive() {
        let rig = rig();
        rig.on(1, "c_1", Some(24_000)).unwrap();
        // Empty, odd and over the most: refused at the door's hand-over, so
        // the stream's own idle clock runs out while they keep coming.
        assert!(!rig.speaker.frame(1, &framed(&[])));
        assert!(!rig.speaker.frame(1, &framed(&[1, 2, 3])));
        assert!(
            !rig.speaker
                .frame(1, &framed(&vec![0u8; FRAME_MAX_BYTES + 2]))
        );
        assert!(wait(|| {
            rig.speaker.frame(1, &framed(&[7]));
            !rig.told_ev("speaker.ended").is_empty()
        }));
        assert_eq!(rig.told_ev("speaker.ended")[0]["why"], "idle");
        assert!(rig.blocks.lock().unwrap().is_empty());
        // The same bad sizes count in `stray`.
        let rig = self::rig();
        rig.on(1, "c_1", Some(24_000)).unwrap();
        rig.speaker.frame(1, &framed(&[1]));
        rig.speaker.frame(1, &framed(&[]));
        rig.speaker.frame(1, &framed(&pcm(480)));
        assert!(wait(|| {
            rig.told_ev("speaker.stats")
                .last()
                .is_some_and(|stats| stats["stray"] == 2)
        }));
    }

    #[test]
    fn a_message_with_no_header_of_ours_is_stray_and_a_framed_one_plays_only_its_samples() {
        let rig = rig();
        rig.on(1, "c_1", Some(24_000)).unwrap();
        let sound = pcm(480);
        // Bare samples (the first four bytes are no length of ours), a length
        // of 0, a header that is no JSON, one without a `sequence`, another
        // event's, a negative `sequence`, a length past the message and one
        // over the most.
        let mut past = 1_000u32.to_be_bytes().to_vec();
        past.extend_from_slice(&sound);
        let bad = [
            sound.clone(),
            framed_with(b"", &sound),
            framed_with(b"not json at all", &sound),
            framed_with(br#"{"ev":"speaker.audio"}"#, &sound),
            framed_with(br#"{"ev":"microphone.audio","sequence":1}"#, &sound),
            framed_with(br#"{"ev":"speaker.audio","sequence":-1}"#, &sound),
            past,
            framed_with(&vec![b' '; HEADER_MAX_BYTES + 1], &sound),
        ];
        for frame in &bad {
            assert!(!rig.speaker.frame(1, frame));
        }
        assert!(rig.blocks.lock().unwrap().is_empty());
        // One of the documented frame plays its samples and not its header.
        assert!(rig.speaker.frame(
            1,
            &framed_with(br#"{"ev":"speaker.audio","sequence":9}"#, &sound)
        ));
        assert!(wait(|| rig.blocks.lock().unwrap().len() == 1));
        let blocks = rig.blocks.lock().unwrap();
        let (_, samples) = lamp_msgs::Audio::split(&blocks[0]).unwrap();
        assert_eq!(samples, &sound[..], "the header is not played as sound");
        drop(blocks);
        assert!(wait(|| {
            rig.told_ev("speaker.stats")
                .last()
                .is_some_and(|stats| stats["stray"] == bad.len())
        }));
    }

    #[test]
    fn a_frame_when_no_stream_runs_goes_nowhere_and_says_nothing() {
        let rig = rig();
        rig.speaker.frame(1, &framed(&pcm(480)));
        assert!(rig.blocks.lock().unwrap().is_empty());
        assert!(rig.told.lock().unwrap().is_empty());
    }

    #[test]
    fn a_second_on_replaces_the_first_and_the_old_computer_is_told() {
        let rig = rig();
        rig.on(1, "c_1", Some(24_000)).unwrap();
        rig.on(2, "c_2", Some(24_000)).unwrap();
        let ended = rig.told_ev("speaker.ended");
        assert_eq!(ended.len(), 1);
        assert_eq!(ended[0]["why"], REPLACED);
        // The old connection's late frames go nowhere; the new one's go.
        rig.speaker.frame(1, &framed(&pcm(480)));
        rig.speaker.frame(2, &framed(&pcm(480)));
        assert!(wait(|| rig.blocks.lock().unwrap().len() == 1));
    }

    #[test]
    fn a_full_queue_drops_the_newest_and_the_gap_shows_in_the_sequence() {
        let rig = rig();
        // The stream thread is held up by a publisher that waits.
        let (hold, release) = mpsc::channel::<()>();
        let release = Arc::new(Mutex::new(release));
        let blocks = rig.blocks.clone();
        let slow: Publish = Arc::new(move |payload| {
            let _ = release.lock().unwrap().recv_timeout(Duration::from_secs(2));
            blocks.lock().unwrap().push(payload.to_vec());
            Ok(())
        });
        let speaker = Speaker::new(Arc::new(|| false), Arc::new(|| false))
            .playing(Arc::new(FakeVoice(rig.said.clone())), slow)
            .marking(Arc::new(|_, _| {}), Arc::new(|_| Some(Instant::now())))
            .switched(true)
            .timing(Duration::from_secs(5), Duration::from_secs(5));
        speaker
            .stream(1, "c_1", "a", true, Some(24_000), rig.events())
            .unwrap();
        for _ in 0..QUEUED * 3 {
            speaker.frame(1, &framed(&pcm(240)));
        }
        for _ in 0..QUEUED * 3 {
            let _ = hold.send(());
        }
        assert!(wait(|| rig.blocks.lock().unwrap().len() >= 5));
        let lost = speaker
            .open
            .slot
            .lock()
            .unwrap()
            .as_ref()
            .map(|s| s.counts.lost.load(Ordering::Relaxed));
        assert!(lost.is_some_and(|lost| lost >= QUEUED as u64), "{lost:?}");
    }

    #[test]
    fn it_ends_at_once_when_the_lamp_turns_private_or_muted() {
        let rig = rig();
        rig.on(1, "c_1", Some(24_000)).unwrap();
        rig.private.store(true, Ordering::SeqCst);
        assert!(wait(|| !rig.told_ev("speaker.ended").is_empty()));
        assert_eq!(rig.told_ev("speaker.ended")[0]["why"], PRIVATE);
        assert!(
            rig.said.closed.load(Ordering::SeqCst),
            "the voice's stream is closed"
        );
        assert!(!rig.speaker.streaming("c_1"));
        rig.private.store(false, Ordering::SeqCst);
        rig.on(1, "c_1", Some(24_000)).unwrap();
        rig.muted.store(true, Ordering::SeqCst);
        assert!(wait(|| rig.told_ev("speaker.ended").len() == 2));
        assert_eq!(rig.told_ev("speaker.ended")[1]["why"], MUTED);
    }

    #[test]
    fn nothing_leaves_until_the_ring_says_it_drew_the_mark_and_no_word_ends_it() {
        let mut rig = rig();
        // Idle waits longer than the ring's second here.
        rig.speaker = rig
            .speaker
            .timing(Duration::from_secs(5), Duration::from_secs(5));
        rig.drawn.store(false, Ordering::SeqCst);
        rig.on(1, "c_1", Some(24_000)).unwrap();
        rig.speaker.frame(1, &framed(&pcm(480)));
        std::thread::sleep(Duration::from_millis(150));
        assert!(
            rig.blocks.lock().unwrap().is_empty(),
            "held back for the mark"
        );
        // No word within a second: it ends, saying why.
        let rig2 = rig;
        assert!(wait(|| !rig2.told_ev("speaker.ended").is_empty()));
        assert_eq!(rig2.told_ev("speaker.ended")[0]["why"], MARK_NOT_SHOWN);
    }

    #[test]
    fn a_stream_with_no_frame_ends_idle_and_the_voice_ending_it_is_passed_on() {
        let rig = rig();
        rig.on(1, "c_1", Some(24_000)).unwrap();
        assert!(wait(|| !rig.told_ev("speaker.ended").is_empty()));
        assert_eq!(rig.told_ev("speaker.ended")[0]["why"], "idle");
        let rig = self::rig();
        rig.on(1, "c_1", Some(24_000)).unwrap();
        rig.said
            .notes
            .lock()
            .unwrap()
            .push_back(Note::Ended("the lamp has something to say".into()));
        assert!(wait(|| !rig.told_ev("speaker.ended").is_empty()));
        assert_eq!(
            rig.told_ev("speaker.ended")[0]["why"],
            "the lamp has something to say"
        );
    }

    #[test]
    fn stats_carry_the_voices_numbers_and_the_counts() {
        let rig = rig();
        rig.on(1, "c_1", Some(24_000)).unwrap();
        rig.said.notes.lock().unwrap().push_back(Note::Stats {
            buffered_ms: 180,
            underruns: 2,
            limited: 1,
        });
        rig.speaker.frame(1, &framed(&pcm(480)));
        assert!(wait(|| {
            rig.told_ev("speaker.stats")
                .last()
                .is_some_and(|stats| stats["buffered_ms"] == 180 && stats["underruns"] == 2)
        }));
        let stats = rig.told_ev("speaker.stats").pop().unwrap();
        assert_eq!(
            (stats["limited"].as_u64(), stats["stray"].as_u64()),
            (Some(1), Some(0))
        );
    }

    #[test]
    fn its_own_off_and_a_revoke_end_it_without_an_event() {
        let rig = rig();
        rig.on(1, "c_1", Some(24_000)).unwrap();
        let answer = rig
            .speaker
            .stream(1, "c_1", "a", false, None, rig.events())
            .unwrap();
        assert_eq!(answer, json!({"stream": "off"}));
        assert!(!rig.speaker.streaming("c_1"));
        rig.on(1, "c_1", Some(24_000)).unwrap();
        rig.speaker.end_client("c_1");
        assert!(wait(|| rig.said.closed.load(Ordering::SeqCst)));
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            rig.told_ev("speaker.ended").is_empty(),
            "no event for its own end"
        );
    }
}
