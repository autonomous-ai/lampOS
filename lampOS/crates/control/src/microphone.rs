//! The microphone through the door: `microphone.stream` (the `microphone`
//! grant, docs/control.md, "The microphone").
//!
//! The door never opens the microphone. The ears own it, and with
//! `LAMP_BUS_AUDIO=on` they publish what it hears on `mic1/audio`
//! (`lamp_msgs::Audio`: mono 16 kHz S16LE, one 320-sample block every
//! 20 ms). The door subscribes to that topic only while a stream is on, on
//! one thread for every stream, and lets the subscription go when the last
//! one ends, so an idle door costs nothing. Each block goes to each stream
//! as one binary WebSocket frame ([`crate::camera::binary_frame`]), through
//! the same sink as the camera's fast stream.
//!
//! Off unless the owner switches it on (`--microphone on`). Never while the
//! lamp is private, and never while someone else may be in the room: the
//! door can't tell the owner from anyone else, so more than one person in
//! view is someone else. Nor while the room is unseen: a face count with no
//! fresh `humans/faces/tracked` behind it (`watch::FACES_STALE_US`) may be
//! frozen, so it counts as unknown, and closed. Each ends every stream at
//! once with a `microphone.ended` event, and refuses a new one. Never unmarked: the
//! ring's mark is asked before the first block leaves and renewed while it
//! runs, and a block leaves only once the ring has said it drew the mark
//! (`light/mark_drawn`, [`crate::camera::shown`]), as the camera's pictures
//! do. Blocks held back for it count in the next one's `lost`; no word from
//! the ring within a second ends the stream.
use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::camera::{Drawn, MARK_EVERY, Mark, Private, Shown, Sink, binary_frame, shown};
use crate::parts::{Events, Media};
use crate::protocol::{Error, ErrorCode};

/// The topic the ears publish the room's sound on.
pub const TOPIC: &str = "mic1/audio";
/// What a stream's answer promises: the ears' own block.
pub const SAMPLE_RATE: u32 = 16_000;
pub const BLOCK_MS: u64 = 20;
/// A stream with no block this long ends, saying so: the ears don't share
/// their sound here (`LAMP_BUS_AUDIO` off), or they stopped.
pub const QUIET_AFTER: Duration = Duration::from_secs(2);
/// Why a stream ended, or was refused, with no sound to give.
pub const NOT_SHARED: &str = "the lamp's microphone isn't shared here";
/// Why a stream is refused on a lamp whose owner hasn't switched it on.
pub const SWITCHED_OFF: &str =
    "the microphone stream is off on this lamp (LAMP_CONTROL_MICROPHONE)";
/// Why a stream is refused, or ends, with more than one person in view: the
/// door can't tell the owner from anyone else, so a second person is
/// someone else.
pub const SOMEONE_ELSE: &str = "someone else is here";
/// Why a stream is refused, or ends, with no fresh word from the eyes on
/// who is in view (`watch::FACES_STALE_US`): the room is unknown.
pub const ROOM_UNSEEN: &str = "the room isn't seen";
/// Why a stream ends with no word from the ring that it drew the mark.
pub const MARK_NOT_SHOWN: &str = "the ring is not showing the microphone's mark";
const PRIVATE: &str = "the lamp is private";
/// How long the stream thread waits for a block before it looks at the
/// privacy switch and the quiet streams again.
const LOOK_EVERY: Duration = Duration::from_millis(50);

/// Where the sound comes from: each block's payload as the ears made it
/// (`lamp_msgs::Audio::block`), or none within `wait`.
pub trait Heard: Send {
    fn next(&mut self, wait: Duration) -> io::Result<Option<Vec<u8>>>;
}

impl Heard for lamp_bus::Subscriber {
    fn next(&mut self, wait: Duration) -> io::Result<Option<Vec<u8>>> {
        Ok(self.recv(wait)?.map(|message| message.payload))
    }
}

/// Opens the source when a first stream starts. Dropped when the last ends.
pub type Listen = Arc<dyn Fn() -> io::Result<Box<dyn Heard>> + Send + Sync>;

/// [`TOPIC`] on the bus, subscribed as `control`.
pub fn on_bus(bus: lamp_bus::Bus) -> Listen {
    Arc::new(move || {
        let subscriber = bus.subscribe(TOPIC, "control")?;
        Ok(Box::new(subscriber) as Box<dyn Heard>)
    })
}

/// One client's stream.
struct Stream {
    /// Which of this client's streams it is: a new `on` replaces it.
    turn: u64,
    /// The last block's `sequence` sent or dropped here.
    last: Option<u64>,
    /// Blocks lost since the last one sent: gaps in `sequence`, and blocks
    /// the client's connection had no room for.
    owed: u64,
    marked_at: Option<Instant>,
    /// When it started: the ring's word on its mark counts from then.
    started: Instant,
    /// When it started, or its last block came.
    heard_at: Instant,
}

#[derive(Default)]
struct Streams {
    each: HashMap<String, Stream>,
    /// Whether the thread that holds the subscription runs.
    listening: bool,
    turns: u64,
    /// Each client's newest stream's turn, kept after that stream ends by
    /// itself (quiet, private, the room unseen, no mark) until the client's
    /// own `off`: an older stream's late `microphone.ended` is then told from
    /// its own (Second's P2 on #5839; the camera's is #5901).
    newest: HashMap<String, u64>,
}

/// A block for one stream, decided under the lock and sent after it.
struct Planned {
    client: String,
    turn: u64,
    lost: u64,
    mark: bool,
    started: Instant,
}

/// How many people are in view now, by a fresh face count; none when the
/// room is unseen (`crate::watch::Watch::room`).
pub type Room = Arc<dyn Fn() -> Option<usize> + Send + Sync>;

/// What stops the microphone, asked before a stream starts and before each
/// block leaves: the lamp private, the room unseen, or someone else in view.
#[derive(Clone)]
struct Gates {
    private: Private,
    room: Room,
}

impl Gates {
    /// Why no sound may leave now, if it may not.
    fn closed(&self) -> Option<(ErrorCode, &'static str)> {
        if (self.private)() {
            Some((ErrorCode::Private, PRIVATE))
        } else {
            match (self.room)() {
                None => Some((ErrorCode::Refused, ROOM_UNSEEN)),
                Some(people) if people > 1 => Some((ErrorCode::Refused, SOMEONE_ELSE)),
                Some(_) => None,
            }
        }
    }
}

/// The microphone as the door reads it: the ears' blocks on the bus,
/// whether the lamp is private or someone else is here, and each client's
/// stream.
pub struct Microphone {
    listen: Listen,
    gates: Gates,
    sink: Sink,
    /// The owner's switch (`--microphone on`); off, every stream is refused.
    switched: bool,
    /// The ring's mark, and its word that it drew it; none, no microphone at
    /// all (it is never heard unmarked).
    mark: Option<(Mark, Drawn)>,
    streams: Arc<Mutex<Streams>>,
    quiet_after: Duration,
}

impl Microphone {
    pub fn new(listen: Listen, private: Private, sink: Sink) -> Self {
        Self {
            listen,
            gates: Gates {
                private,
                // Unknown until a room is given: closed.
                room: Arc::new(|| None),
            },
            sink,
            switched: false,
            mark: None,
            streams: Arc::default(),
            quiet_after: QUIET_AFTER,
        }
    }

    /// The ring's mark while it is heard through, and its word that it drew
    /// it: a block goes only once that word has come.
    pub fn marking(mut self, mark: Mark, drawn: Drawn) -> Self {
        self.mark = Some((mark, drawn));
        self
    }

    /// The owner's switch: off unless set.
    pub fn switched(mut self, on: bool) -> Self {
        self.switched = on;
        self
    }

    /// How many people are in view now, none while the room is unseen.
    pub fn room(mut self, room: Room) -> Self {
        self.gates.room = room;
        self
    }

    /// Another wait before a stream with no block ends, for tests.
    pub fn quiet_after(mut self, after: Duration) -> Self {
        self.quiet_after = after;
        self
    }

    /// `microphone.stream`, on or off. On again while one runs replaces it.
    pub fn stream(&mut self, client: &str, on: bool, events: Events) -> Result<Value, Error> {
        self.end(client);
        if !on {
            return Ok(json!({"stream": "off"}));
        }
        if !self.switched {
            return Err(Error::new(ErrorCode::Refused, SWITCHED_OFF));
        }
        if let Some((code, why)) = self.gates.closed() {
            return Err(Error::new(code, why));
        }
        let mark = self.mark.clone().ok_or_else(|| {
            Error::new(
                ErrorCode::Refused,
                "the microphone is not shown on the ring here, so it is not given",
            )
        })?;
        let sink = self
            .sink
            .get()
            .cloned()
            .ok_or_else(|| Error::new(ErrorCode::Refused, NOT_SHARED))?;
        let mut streams = self.streams.lock().expect("streams");
        if !streams.listening {
            let heard = (self.listen)().map_err(|_| Error::new(ErrorCode::Refused, NOT_SHARED))?;
            streams.listening = true;
            let (shared, gates, quiet_after) =
                (self.streams.clone(), self.gates.clone(), self.quiet_after);
            std::thread::spawn(move || {
                listen(heard, &shared, &gates, &mark, &sink, &events, quiet_after);
            });
        }
        streams.turns += 1;
        let turn = streams.turns;
        streams.newest.insert(client.to_owned(), turn);
        let now = Instant::now();
        streams.each.insert(
            client.to_owned(),
            Stream {
                turn,
                last: None,
                owed: 0,
                marked_at: None,
                started: now,
                heard_at: now,
            },
        );
        Ok(json!({
            "stream": "on", "sample_rate": SAMPLE_RATE, "channels": 1, "format": "s16le",
            "block_ms": BLOCK_MS, "source": "mic1",
        }))
    }

    /// Whether `client`'s stream runs now, and with a `turn`, whether it is
    /// that very stream: a block already queued for it goes only then, never
    /// through a stream that replaced it (Second's P1 on #5839).
    pub fn streaming(&self, client: &str, turn: Option<u64>) -> bool {
        self.streams
            .lock()
            .expect("streams")
            .each
            .get(client)
            .is_some_and(|stream| turn.is_none_or(|turn| stream.turn == turn))
    }

    /// The turn of `client`'s newest stream, running or ended by itself,
    /// until its own `off`: a `microphone.ended` of an older turn is not this
    /// stream's end, and the writer drops it.
    pub fn newest_turn(&self, client: &str) -> Option<u64> {
        self.streams
            .lock()
            .expect("streams")
            .newest
            .get(client)
            .copied()
    }

    /// Ends `client`'s stream, if any, without an event: its own `off`,
    /// `stop`, going, or a re-pairing without the microphone.
    pub fn end(&mut self, client: &str) {
        let mut streams = self.streams.lock().expect("streams");
        streams.each.remove(client);
        streams.newest.remove(client);
    }
}

/// The one thread that holds the subscription: each block to each stream,
/// until none is left. Events and frames go out with the lock let go, since
/// sending them takes the server's own locks.
fn listen(
    mut heard: Box<dyn Heard>,
    streams: &Mutex<Streams>,
    gates: &Gates,
    (mark, drawn): &(Mark, Drawn),
    sink: &crate::parts::Binary,
    events: &Events,
    quiet_after: Duration,
) {
    let ended = |gone: Vec<(String, u64)>, why: &str| {
        for (client, turn) in gone {
            events(&client, ended_event(why, turn));
        }
    };
    let end_all = |why: &str| {
        let gone: Vec<(String, u64)> = {
            let mut streams = streams.lock().expect("streams");
            streams.listening = false;
            streams
                .each
                .drain()
                .map(|(client, stream)| (client, stream.turn))
                .collect()
        };
        ended(gone, why);
    };
    loop {
        if let Some((_, why)) = gates.closed() {
            return end_all(why);
        }
        let block = match heard.next(LOOK_EVERY) {
            Ok(Some(payload)) => lamp_msgs::Audio::split(&payload)
                .ok()
                .map(|(audio, samples)| (audio, samples.to_vec())),
            Ok(None) => None,
            Err(_) => {
                std::thread::sleep(LOOK_EVERY);
                None
            }
        };
        let now = Instant::now();
        let (sends, quiet, last) = {
            let mut streams = streams.lock().expect("streams");
            let mut sends = Vec::new();
            if let Some((audio, _)) = &block {
                for (client, stream) in &mut streams.each {
                    let gap = stream
                        .last
                        .map_or(0, |last| audio.sequence.saturating_sub(last + 1));
                    stream.last = Some(audio.sequence);
                    stream.owed += gap;
                    stream.heard_at = now;
                    let mark = stream
                        .marked_at
                        .is_none_or(|at| now.duration_since(at) >= MARK_EVERY);
                    if mark {
                        stream.marked_at = Some(now);
                    }
                    sends.push(Planned {
                        client: client.clone(),
                        turn: stream.turn,
                        lost: stream.owed,
                        mark,
                        started: stream.started,
                    });
                }
            }
            let quiet: Vec<(String, u64)> = streams
                .each
                .iter()
                .filter(|(_, stream)| now.duration_since(stream.heard_at) >= quiet_after)
                .map(|(client, stream)| (client.clone(), stream.turn))
                .collect();
            for (client, _) in &quiet {
                streams.each.remove(client);
            }
            let last = streams.each.is_empty();
            if last {
                streams.listening = false;
            }
            (sends, quiet, last)
        };
        ended(quiet, NOT_SHARED);
        if let Some((audio, samples)) = &block {
            for send in sends {
                // Checked again before each block leaves.
                if let Some((_, why)) = gates.closed() {
                    return end_all(why);
                }
                // The mark is asked before the sound leaves, and the block
                // goes only once the ring has said it drew it.
                if send.mark {
                    mark(&send.client, false);
                }
                let taken = match shown(drawn, &send.client, send.started) {
                    Shown::Yes => {
                        let header = json!({
                            "ev": "microphone.audio", "sequence": audio.sequence, "lost": send.lost,
                            "sample_rate": audio.sample_rate, "channels": audio.channels,
                            "format": audio.sample_format.to_ascii_lowercase(),
                            "bytes": samples.len(),
                        });
                        still(streams, &send)
                            && sink(
                                &send.client,
                                Media::Microphone,
                                Some(send.turn),
                                &binary_frame(&header, samples),
                            )
                    }
                    // Held back until the ring's word: counted as lost.
                    Shown::NotYet => false,
                    Shown::Never => {
                        if end_one(streams, &send, MARK_NOT_SHOWN, events) {
                            return;
                        }
                        continue;
                    }
                };
                let mut streams = streams.lock().expect("streams");
                if let Some(stream) = streams.each.get_mut(&send.client)
                    && stream.turn == send.turn
                {
                    // Sent: nothing owed. Not taken: this block is lost too.
                    stream.owed = if taken { 0 } else { send.lost + 1 };
                }
            }
        }
        if last {
            return;
        }
    }
}

/// Ends the stream `send` was meant for, if it still runs, saying `why`:
/// whether no stream is left, the subscription then let go.
fn end_one(streams: &Mutex<Streams>, send: &Planned, why: &str, events: &Events) -> bool {
    let (ended, none_left) = {
        let mut streams = streams.lock().expect("streams");
        let ours = streams
            .each
            .get(&send.client)
            .is_some_and(|stream| stream.turn == send.turn);
        if ours {
            streams.each.remove(&send.client);
        }
        let none_left = streams.each.is_empty();
        if none_left {
            streams.listening = false;
        }
        (ours, none_left)
    };
    if ended {
        events(&send.client, ended_event(why, send.turn));
    }
    none_left
}

/// A stream's `microphone.ended`, with its turn in the door's own field
/// (`crate::camera::TURN_FIELD`), which the writer takes off before it goes.
fn ended_event(why: &str, turn: u64) -> Value {
    json!({"ev": "microphone.ended", "why": why, crate::camera::TURN_FIELD: turn})
}

/// Whether the stream a block was meant for still runs.
fn still(streams: &Mutex<Streams>, send: &Planned) -> bool {
    streams
        .lock()
        .expect("streams")
        .each
        .get(&send.client)
        .is_some_and(|stream| stream.turn == send.turn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;

    /// The ears' side in a test: blocks pushed by the test.
    struct Blocks(mpsc::Receiver<Vec<u8>>);

    impl Heard for Blocks {
        fn next(&mut self, wait: Duration) -> io::Result<Option<Vec<u8>>> {
            match self.0.recv_timeout(wait) {
                Ok(payload) => Ok(Some(payload)),
                Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    std::thread::sleep(wait);
                    Ok(None)
                }
            }
        }
    }

    fn block(sequence: u64) -> Vec<u8> {
        lamp_msgs::Audio {
            sample_rate: 16_000,
            channels: 1,
            sample_format: lamp_msgs::Audio::S16LE.to_owned(),
            sequence,
        }
        .block(&[7u8; 640])
        .unwrap()
    }

    /// What a test sees: each frame sent, each event, each mark asked, in
    /// one order.
    #[derive(Debug, Clone, PartialEq)]
    enum Seen {
        Frame(Value, usize),
        Event(Value),
        Mark(String),
    }

    struct Rig {
        microphone: Microphone,
        ears: mpsc::Sender<Vec<u8>>,
        seen: Arc<Mutex<Vec<Seen>>>,
        private: Arc<AtomicBool>,
        /// People in view by a fresh count, none while the room is unseen.
        in_view: Arc<Mutex<Option<usize>>>,
        listening: Arc<Mutex<u32>>,
        room: Arc<AtomicBool>,
        /// Whether the ring says it drew the mark: from when this is set.
        draws: Arc<Mutex<Option<Instant>>>,
    }

    fn rig() -> Rig {
        let (ears, blocks) = mpsc::channel();
        let blocks = Arc::new(Mutex::new(Some(blocks)));
        let listening: Arc<Mutex<u32>> = Arc::default();
        let opened = listening.clone();
        let listen: Listen = Arc::new(move || {
            *opened.lock().unwrap() += 1;
            // The first subscription gets the test's blocks; a later one (a
            // stream after the last ended) a source with nothing in it.
            let blocks = blocks
                .lock()
                .unwrap()
                .take()
                .unwrap_or_else(|| mpsc::channel().1);
            Ok(Box::new(Blocks(blocks)) as Box<dyn Heard>)
        });
        let private = Arc::new(AtomicBool::new(false));
        let watching = private.clone();
        let in_view = Arc::new(Mutex::new(Some(1)));
        let counted = in_view.clone();
        let seen: Arc<Mutex<Vec<Seen>>> = Arc::default();
        let (into, marks) = (seen.clone(), seen.clone());
        let room = Arc::new(AtomicBool::new(true));
        let has_room = room.clone();
        let draws: Arc<Mutex<Option<Instant>>> =
            Arc::new(Mutex::new(Some(Instant::now() - Duration::from_secs(1))));
        let sink: Sink = Arc::default();
        let _ = sink.set(Arc::new(
            move |client: &str, media, turn: Option<u64>, frame: &[u8]| {
                assert_eq!(client, "c_1");
                assert_eq!(media, Media::Microphone);
                assert!(turn.is_some(), "a block is sent with its stream's turn");
                if !has_room.load(Ordering::SeqCst) {
                    return false;
                }
                let length = u32::from_be_bytes(frame[..4].try_into().unwrap()) as usize;
                let header: Value = serde_json::from_slice(&frame[4..4 + length]).unwrap();
                into.lock()
                    .unwrap()
                    .push(Seen::Frame(header, frame.len() - 4 - length));
                true
            },
        ));
        let microphone = Microphone::new(
            listen,
            Arc::new(move || watching.load(Ordering::SeqCst)),
            sink,
        )
        .switched(true)
        .room(Arc::new(move || *counted.lock().unwrap()))
        .marking(
            Arc::new(move |client: &str, blink| {
                assert!(!blink);
                marks.lock().unwrap().push(Seen::Mark(client.to_owned()));
            }),
            {
                // A ring that draws: says now, from the moment it began.
                let draws = draws.clone();
                Arc::new(move |_: &str| {
                    draws
                        .lock()
                        .unwrap()
                        .filter(|since| *since <= Instant::now())
                        .map(|_| Instant::now())
                })
            },
        );
        Rig {
            microphone,
            ears,
            seen,
            private,
            in_view,
            listening,
            room,
            draws,
        }
    }

    impl Rig {
        fn events(&self) -> Events {
            let into = self.seen.clone();
            Arc::new(move |_, event| into.lock().unwrap().push(Seen::Event(event)))
        }

        fn frames(&self) -> Vec<Value> {
            self.seen
                .lock()
                .unwrap()
                .iter()
                .filter_map(|seen| match seen {
                    Seen::Frame(header, _) => Some(header.clone()),
                    _ => None,
                })
                .collect()
        }

        fn last_event(&self) -> Option<Value> {
            self.seen
                .lock()
                .unwrap()
                .iter()
                .rev()
                .find_map(|seen| match seen {
                    Seen::Event(event) => Some(event.clone()),
                    _ => None,
                })
                .map(|mut event| {
                    // As the writer sends it: the turn is the door's own.
                    if let Some(fields) = event.as_object_mut() {
                        fields.remove(crate::camera::TURN_FIELD);
                    }
                    event
                })
        }

        fn last_event_turn(&self) -> Option<u64> {
            self.seen
                .lock()
                .unwrap()
                .iter()
                .rev()
                .find_map(|seen| match seen {
                    Seen::Event(event) => event[crate::camera::TURN_FIELD].as_u64(),
                    _ => None,
                })
        }
    }

    fn settle() {
        std::thread::sleep(Duration::from_millis(150));
    }

    #[test]
    fn nothing_is_heard_while_private() {
        let mut rig = rig();
        rig.private.store(true, Ordering::SeqCst);
        let events = rig.events();
        let refused = rig.microphone.stream("c_1", true, events).unwrap_err();
        assert_eq!(refused.code, ErrorCode::Private);
        assert_eq!(*rig.listening.lock().unwrap(), 0, "never subscribed");
    }

    #[test]
    fn with_no_mark_there_is_no_microphone() {
        let rig = rig();
        let mut unmarked = Microphone::new(
            rig.microphone.listen.clone(),
            Arc::new(|| false),
            rig.microphone.sink.clone(),
        )
        .switched(true)
        .room(Arc::new(|| Some(1)));
        let refused = unmarked.stream("c_1", true, rig.events()).unwrap_err();
        assert_eq!(refused.code, ErrorCode::Refused);
        assert!(
            refused.why.contains("not shown on the ring"),
            "{}",
            refused.why
        );
        assert_eq!(*rig.listening.lock().unwrap(), 0, "never subscribed");
    }

    #[test]
    fn each_block_goes_as_a_binary_frame_marked_before_the_first() {
        let mut rig = rig();
        let events = rig.events();
        assert_eq!(
            rig.microphone.stream("c_1", true, events).unwrap(),
            json!({"stream": "on", "sample_rate": 16000, "channels": 1, "format": "s16le",
                   "block_ms": 20, "source": "mic1"})
        );
        assert_eq!(*rig.listening.lock().unwrap(), 1);
        for sequence in 10..13 {
            rig.ears.send(block(sequence)).unwrap();
        }
        settle();
        let seen = rig.seen.lock().unwrap().clone();
        // The mark first, then the sound; renewed each second, not each block.
        assert_eq!(seen[0], Seen::Mark("c_1".to_owned()));
        assert_eq!(
            seen[1],
            Seen::Frame(
                json!({"ev": "microphone.audio", "sequence": 10, "lost": 0, "sample_rate": 16000,
                       "channels": 1, "format": "s16le", "bytes": 640}),
                640
            )
        );
        assert_eq!(seen.len(), 4, "{seen:?}");
        // Off: no event, and the subscription goes with the last stream.
        assert_eq!(
            rig.microphone.stream("c_1", false, rig.events()).unwrap(),
            json!({"stream": "off"})
        );
        rig.ears.send(block(13)).unwrap();
        settle();
        assert_eq!(rig.frames().len(), 3);
        assert_eq!(rig.last_event(), None);
        assert!(!rig.microphone.streams.lock().unwrap().listening);
    }

    /// Thirteen's P2 on #4521, for the microphone: a block leaves only once
    /// the ring says it drew the mark; those held back before count in the
    /// next one's `lost`.
    #[test]
    fn blocks_wait_for_the_rings_word_on_the_mark_and_count_as_lost() {
        let mut rig = rig();
        *rig.draws.lock().unwrap() = None;
        rig.microphone.stream("c_1", true, rig.events()).unwrap();
        for sequence in 1..4 {
            rig.ears.send(block(sequence)).unwrap();
        }
        settle();
        assert!(rig.frames().is_empty(), "no word yet, no sound");
        assert!(
            rig.seen
                .lock()
                .unwrap()
                .contains(&Seen::Mark("c_1".to_owned())),
            "the mark was asked"
        );
        *rig.draws.lock().unwrap() = Some(Instant::now());
        rig.ears.send(block(4)).unwrap();
        settle();
        let frames = rig.frames();
        assert_eq!(frames.len(), 1, "{frames:?}");
        assert_eq!(
            (frames[0]["sequence"].as_u64(), frames[0]["lost"].as_u64()),
            (Some(4), Some(3))
        );
        assert!(rig.microphone.streaming("c_1", None));
        rig.microphone.end("c_1");
        assert!(!rig.microphone.streaming("c_1", None));
    }

    /// Second's P1 on #5839: a block queued by a stream that ended (an `off`,
    /// then an `on`) must not go through the stream that replaced it, whose
    /// own mark the ring may not have drawn yet. The queue holds each block's
    /// turn, and only that very stream lets it go.
    #[test]
    fn a_block_of_an_ended_stream_never_goes_through_the_one_that_replaced_it() {
        let mut rig = rig();
        rig.microphone.stream("c_1", true, rig.events()).unwrap();
        let first = rig.microphone.streams.lock().unwrap().each["c_1"].turn;
        assert!(rig.microphone.streaming("c_1", Some(first)));
        rig.microphone.stream("c_1", false, rig.events()).unwrap();
        assert!(!rig.microphone.streaming("c_1", Some(first)));
        rig.microphone.stream("c_1", true, rig.events()).unwrap();
        let second = rig.microphone.streams.lock().unwrap().each["c_1"].turn;
        assert_ne!(first, second);
        assert!(
            !rig.microphone.streaming("c_1", Some(first)),
            "the ended stream's block must not go through its replacement"
        );
        assert!(rig.microphone.streaming("c_1", Some(second)));
        rig.microphone.end("c_1");
        assert!(!rig.microphone.streaming("c_1", Some(second)));
    }

    /// No word from the ring within a second: the stream ends, saying so,
    /// with nothing heard, and the subscription goes with it.
    #[test]
    fn a_stream_the_ring_never_marks_ends_with_nothing_heard() {
        let mut rig = rig();
        *rig.draws.lock().unwrap() = None;
        rig.microphone.stream("c_1", true, rig.events()).unwrap();
        let until = Instant::now() + crate::camera::MARK_SHOWN_WITHIN + Duration::from_millis(300);
        let mut sequence = 0;
        while Instant::now() < until {
            sequence += 1;
            // Once the stream has ended, its thread has let the ears go.
            let _ = rig.ears.send(block(sequence));
            std::thread::sleep(Duration::from_millis(50));
        }
        settle();
        assert!(rig.frames().is_empty());
        assert_eq!(
            rig.last_event(),
            Some(json!({"ev": "microphone.ended", "why": MARK_NOT_SHOWN}))
        );
        assert!(!rig.microphone.streaming("c_1", None));
        assert!(!rig.microphone.streams.lock().unwrap().listening);
    }

    #[test]
    fn lost_counts_a_gap_and_the_blocks_a_full_connection_dropped() {
        let mut rig = rig();
        rig.microphone.stream("c_1", true, rig.events()).unwrap();
        rig.ears.send(block(1)).unwrap();
        // Three lost on the bus.
        rig.ears.send(block(5)).unwrap();
        settle();
        // Two the connection had no room for.
        rig.room.store(false, Ordering::SeqCst);
        rig.ears.send(block(6)).unwrap();
        rig.ears.send(block(7)).unwrap();
        settle();
        rig.room.store(true, Ordering::SeqCst);
        rig.ears.send(block(8)).unwrap();
        settle();
        let lost: Vec<(u64, u64)> = rig
            .frames()
            .iter()
            .map(|header| {
                (
                    header["sequence"].as_u64().unwrap(),
                    header["lost"].as_u64().unwrap(),
                )
            })
            .collect();
        assert_eq!(lost, [(1, 0), (5, 3), (8, 2)]);
        rig.microphone.end("c_1");
    }

    #[test]
    fn a_stream_ends_at_once_when_the_lamp_turns_private() {
        let mut rig = rig();
        rig.microphone.stream("c_1", true, rig.events()).unwrap();
        rig.ears.send(block(1)).unwrap();
        settle();
        rig.private.store(true, Ordering::SeqCst);
        rig.ears.send(block(2)).unwrap();
        settle();
        assert_eq!(rig.frames().len(), 1, "nothing after private");
        assert_eq!(
            rig.last_event(),
            Some(json!({"ev": "microphone.ended", "why": "the lamp is private"}))
        );
        assert!(!rig.microphone.streams.lock().unwrap().listening);
    }

    #[test]
    fn a_stream_with_no_sound_ends_saying_the_microphone_isnt_shared() {
        let mut rig = rig();
        rig.microphone = rig.microphone.quiet_after(Duration::from_millis(200));
        rig.microphone.stream("c_1", true, rig.events()).unwrap();
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(
            rig.last_event(),
            Some(json!({"ev": "microphone.ended", "why": NOT_SHARED}))
        );
        assert!(!rig.microphone.streams.lock().unwrap().listening);
    }

    /// Second's P2 on #5839: stream A ends by itself (quiet here) and its
    /// end is told late, after B started. The end carries A's turn, B's turn
    /// is the client's newest, and the writer drops A's end
    /// (`camera::older_end`), while B's own end still goes, even once B has
    /// ended by itself.
    #[test]
    fn an_older_streams_late_end_is_told_from_the_newest_streams_own() {
        let mut rig = rig();
        rig.microphone = rig.microphone.quiet_after(Duration::from_millis(200));
        rig.microphone.stream("c_1", true, rig.events()).unwrap();
        let first = rig.microphone.newest_turn("c_1").unwrap();
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(
            rig.last_event(),
            Some(json!({"ev": "microphone.ended", "why": NOT_SHARED}))
        );
        assert_eq!(
            rig.last_event_turn(),
            Some(first),
            "the end carries its turn"
        );
        // Ended by itself, it stays the newest: its own end goes.
        assert_eq!(rig.microphone.newest_turn("c_1"), Some(first));
        assert!(!crate::camera::older_end(
            Some(first),
            rig.microphone.newest_turn("c_1")
        ));
        // B starts before A's end reaches the writer: A's is dropped.
        rig.microphone.stream("c_1", true, rig.events()).unwrap();
        let second = rig.microphone.newest_turn("c_1").unwrap();
        assert_ne!(first, second);
        assert!(crate::camera::older_end(
            Some(first),
            rig.microphone.newest_turn("c_1")
        ));
        assert!(!crate::camera::older_end(
            Some(second),
            rig.microphone.newest_turn("c_1")
        ));
        // After the computer's own off, an end goes as it is, as the camera's.
        rig.microphone.end("c_1");
        assert_eq!(rig.microphone.newest_turn("c_1"), None);
        assert!(!crate::camera::older_end(Some(first), None));
        assert!(!crate::camera::older_end(None, Some(second)));
    }

    #[test]
    fn off_unless_the_owner_switches_it_on() {
        let mut rig = rig();
        rig.microphone = rig.microphone.switched(false);
        let refused = rig
            .microphone
            .stream("c_1", true, rig.events())
            .unwrap_err();
        assert_eq!(
            (refused.code, refused.why.as_str()),
            (ErrorCode::Refused, SWITCHED_OFF)
        );
        assert_eq!(*rig.listening.lock().unwrap(), 0, "never subscribed");
        // Off is still answered: nothing runs to end.
        assert_eq!(
            rig.microphone.stream("c_1", false, rig.events()).unwrap(),
            json!({"stream": "off"})
        );
    }

    #[test]
    fn never_while_someone_else_is_here() {
        let mut rig = rig();
        *rig.in_view.lock().unwrap() = Some(2);
        let refused = rig
            .microphone
            .stream("c_1", true, rig.events())
            .unwrap_err();
        assert_eq!(
            (refused.code, refused.why.as_str()),
            (ErrorCode::Refused, SOMEONE_ELSE)
        );
        assert_eq!(*rig.listening.lock().unwrap(), 0, "never subscribed");
        // One person (or nobody) in view: it runs, until a second comes.
        *rig.in_view.lock().unwrap() = Some(1);
        rig.microphone.stream("c_1", true, rig.events()).unwrap();
        rig.ears.send(block(1)).unwrap();
        settle();
        *rig.in_view.lock().unwrap() = Some(2);
        rig.ears.send(block(2)).unwrap();
        settle();
        assert_eq!(rig.frames().len(), 1, "nothing after a second person");
        assert_eq!(
            rig.last_event(),
            Some(json!({"ev": "microphone.ended", "why": SOMEONE_ELSE}))
        );
        assert!(!rig.microphone.streams.lock().unwrap().listening);
    }

    /// The local leader's P2: a face count with nothing fresh behind it is
    /// unknown, and closed. A fresh 1 lets a stream run; the count gone
    /// stale ends it, "the room isn't seen", and refuses a new one; a fresh
    /// 2 refuses as someone else.
    #[test]
    fn an_unseen_room_closes_the_microphone_as_a_crowded_one_does() {
        let mut rig = rig();
        rig.microphone.stream("c_1", true, rig.events()).unwrap();
        rig.ears.send(block(1)).unwrap();
        settle();
        assert_eq!(rig.frames().len(), 1, "a fresh 1: heard");
        *rig.in_view.lock().unwrap() = None;
        rig.ears.send(block(2)).unwrap();
        settle();
        assert_eq!(rig.frames().len(), 1, "nothing once the room is unseen");
        assert_eq!(
            rig.last_event(),
            Some(json!({"ev": "microphone.ended", "why": ROOM_UNSEEN}))
        );
        assert!(!rig.microphone.streaming("c_1", None));
        let refused = rig
            .microphone
            .stream("c_1", true, rig.events())
            .unwrap_err();
        assert_eq!(
            (refused.code, refused.why.as_str()),
            (ErrorCode::Refused, ROOM_UNSEEN)
        );
        *rig.in_view.lock().unwrap() = Some(2);
        let refused = rig
            .microphone
            .stream("c_1", true, rig.events())
            .unwrap_err();
        assert_eq!(refused.why, SOMEONE_ELSE);
    }

    /// With no room given at all, the microphone is closed: unknown.
    #[test]
    fn with_no_room_given_the_microphone_is_closed() {
        let rig = rig();
        let mut unwatched = Microphone::new(
            rig.microphone.listen.clone(),
            Arc::new(|| false),
            rig.microphone.sink.clone(),
        )
        .switched(true)
        .marking(Arc::new(|_, _| {}), Arc::new(|_| Some(Instant::now())));
        let refused = unwatched.stream("c_1", true, rig.events()).unwrap_err();
        assert_eq!(refused.why, ROOM_UNSEEN);
        assert_eq!(*rig.listening.lock().unwrap(), 0, "never subscribed");
    }
}
