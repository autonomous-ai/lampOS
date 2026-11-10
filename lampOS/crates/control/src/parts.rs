//! Each action routed to the part of the lamp that already does it
//! (docs/control.md, "Routing"), held to that part's own limits:
//!
//! - `gesture` and `move` → the body's mood socket, as lamp-skills sends
//!   them (`lamp_skills::body_ask`), at `skill` priority with the source
//!   `control:<client>`. The body plays them with its own move check, reach,
//!   speed and anti-tip limits, or drops them with why in its journal.
//!   Each carries `"replace": true`: a computer's new motion interrupts the
//!   one playing (another computer's, a skill's, the lamp's own), which the
//!   body ends at once and eases out of from where it is, under its own
//!   caps, before the new one plays. The person's own use, or the lamp's
//!   conversation, is never interrupted: the body refuses with why "the
//!   person is using the lamp".
//! - `light` → a `light/request` on the bus at `skill` priority, lapsing at
//!   its `for_ms` unless asked again, the ring's own night caps on top.
//!   `stop` gives the light back at once.
//! - `say` → the voice's say socket as written words (`text`), through its
//!   quiet, privacy, pause and busy gates. Its outcome comes later as a
//!   `done` event: spoken, dropped, expired or refused, with why. The
//!   client's `stop` cancels each of its own says on the connection that
//!   asked it ([`SAY_CANCEL`]): waiting, the voice lets it go; playing, the
//!   voice cuts that turn alone. Its `done` is then dropped, "stopped by its
//!   asker". Another client's say, the person's turn and the voice's own
//!   words are never reached. A `sound` is cancelled by `stop` as a say is.
//! - `turn.start` → the voice's say socket as a turn (`turn: "start"`,
//!   with its prompt as `text`), opened as a tap on the head opens one. Its
//!   outcome comes later as a `done`; the turn's id, from the voice's
//!   `begun`, is kept for the computer, so its `turn.end` names its own
//!   turn and no other. It is kept past the outcome, as the turn's
//!   conversation may go on, until a newer turn's `begun` replaces it or a
//!   `turn.end` used it; a start refused before its `begun` leaves it. A
//!   start while one is kept names it as `after`, so the voice continues
//!   that computer's own conversation rather than refusing it.
//!   A start still waiting for its `begun` when the computer is gone (its
//!   last connection, a revoke) has its connection shut, and the voice
//!   drops it: no cue and no window for a computer gone.
//!   `turn.end` → `turn: "end"` with that id, answered once the voice says.
//! - `listen.set` → the voice's say socket as `listen`: `false` holds one
//!   connection for the computer, which holds the voice's mute while it is
//!   open; `true` lifts it on that connection and closes it. The door
//!   closes it too when the computer is gone (its last connection, a
//!   revoke), so a computer gone never leaves the lamp deaf. A hand on the
//!   head lifts it in the voice, told to the computer as a `done`.
//!   A `stop` ends neither a computer's turn (`turn.end` does) nor its
//!   mute (`listen.set true` does).
//! - `express` → the `feelings` skill's own move for it, sent whole on the
//!   mood socket as `move` is, and its light as `light` is
//!   (`crate::feelings`). With no light to show (off, not known, or at its
//!   dimmest with no warmth to add) the move goes alone, and the answer says
//!   why.
//!
//! The priority is this part's, never the caller's: a client cannot claim
//! more than `skill`. With the parts' own caller check (#3720, Ten's caps)
//! the body and the ring hold `lamp-control` to `skill` as well.
//!
//! - `look` at `ahead` or `{yaw, pitch}` → the body's lease socket, under
//!   the lease the door holds for the computer (`crate::body_lease`); its
//!   end comes later as a `done` event, with the angles the head reached.
//!
//! `look` at `face` goes the same way as `{"look":"face"}`: the body turns
//! once toward the face the eyes last showed, by the gaze's measure, and
//! answers `refused` (a `done` `dropped`) with "no face in view" when there
//! is none. `look` at `name` is not in this release (the lamp has no sound
//! direction), and refused as that. `joints`' `yaw` and `pitch` go as the holder's look, the same
//! degrees from home; its `roll`, `shoulder` and `elbow` as the holder's
//! pose, on the same socket. Not routed yet, each refused with what it waits for: `play`
//! and `sound` (a client's package,
//! with the Store's packaging). State and events from the
//! bus come in their own piece.

use std::collections::HashMap;
use std::io::{BufRead as _, BufReader, Write as _};
use std::net::Shutdown;
use std::os::unix::net::{UnixDatagram, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use lamp_msgs::{Claim, Light, LightPattern, LightRequest, PatternKind, Priority};
use serde_json::{Value, json};

use crate::feelings;
use crate::protocol::{
    Angles, Error, ErrorCode, LookAt, LookTarget, Op, Pattern, StateKind, VolumeStep,
};
use crate::session::{Parts, Reply};

/// How long a `say` may wait to begin before the voice lets it go.
pub const SAY_EXPIRES_MS: u64 = 30_000;
/// How long the say socket's answer may take, its turn included.
pub const SAY_ANSWER_WITHIN: Duration = Duration::from_secs(90);
/// The line that cancels a say on the connection that asked it: the
/// voice's `lamp_voice::say::CANCEL_LINE`.
pub const SAY_CANCEL: &str = r#"{"cancel":true}"#;
/// Why a say its client stopped is dropped: the voice's own words for it.
pub const STOPPED_BY_ASKER: &str = "stopped by its asker";
/// The longest the door's lock waits to write a cancel to the voice.
const SAY_CANCEL_WITHIN: Duration = Duration::from_millis(100);
/// One budget for all pending starts' cancellation writes and voice receipts.
/// The voice takes a cancel at its next pass, usually tens of ms.
pub const TURN_CANCEL_WITHIN: Duration = Duration::from_secs(2);

/// The says each client still waits on, by client and then by the say's
/// own number: the connection to the voice once it is open. A say no
/// longer listed was stopped.
type Says = Arc<std::sync::Mutex<HashMap<String, HashMap<u64, Option<UnixStream>>>>>;

/// How long the voice's first answer to a `turn.end` or a `listen` may
/// take: it waits at most 10 s on an end (`say::TURN_END_EXPIRY_MS`) and
/// 5 s to take a mute (`control_mute::TAKE_WITHIN`).
pub const VOICE_ANSWER_WITHIN: Duration = Duration::from_secs(15);
/// What a voice says of a line it doesn't know: a field it doesn't take,
/// so a voice from before turns and a computer's mute.
pub const NOT_ITS_PROTOCOL: &str = "not a say request";
/// A `turn.start` or `turn.end` an older voice refused.
pub const NO_TURNS: &str = "the voice here doesn't take turns from a computer";
/// A `listen.set` an older voice refused.
pub const NO_MUTE: &str = "the voice here doesn't take a computer's mute";
/// A `turn.end` with no turn of this computer's to end, the voice's words.
pub const NOT_THIS_TURN: &str = "not this computer's turn";
/// The `done` of a computer's `turn.start` its own `turn.end` dropped
/// before the voice began it.
pub const ENDED_BEFORE_BEGUN: &str = "ended by this computer before it began";
/// A `listen.set` asked while this computer's last is still with the voice.
pub const MUTE_BEING_TAKEN: &str = "this computer's mute is still being taken by the voice";

/// Where the events of actions that end later go: the client, and the
/// event (`done`).
pub type Events = Arc<dyn Fn(&str, Value) + Send + Sync>;
/// Where a client's binary frames go (`crate::camera`'s fast stream and
/// `crate::microphone`'s blocks): whether every connection of the client
/// had room for it.
/// A binary frame for a client: its kind, the turn of the stream that made
/// it (a camera stream's, #5881, or a microphone stream's), and the bytes. A
/// queued frame goes only while that very stream runs (Second's P1 on #5839).
pub type Binary = Arc<dyn Fn(&str, Media, Option<u64>, &[u8]) -> bool + Send + Sync>;

/// What a binary frame carries: a camera picture or a microphone block.
/// Each goes only to a connection whose computer holds its grant, and a
/// queued one only while that computer's stream of it still runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Media {
    Camera,
    Microphone,
}

impl Media {
    /// The grant its frames need.
    pub fn grant(self) -> crate::protocol::Grant {
        match self {
            Self::Camera => crate::protocol::Grant::Camera,
            Self::Microphone => crate::protocol::Grant::Microphone,
        }
    }
}

/// Something a light request can be published on: the bus, or a test's
/// list.
pub trait LightOut: Send {
    /// Publishes it: how many subscribers took it.
    fn publish(&mut self, request: &LightRequest) -> std::io::Result<usize>;
}

impl LightOut for lamp_bus::Publisher {
    fn publish(&mut self, request: &LightRequest) -> std::io::Result<usize> {
        Ok(self
            .publish_json(lamp_ipc::monotonic_us(), request)?
            .delivered)
    }
}

/// The name a computer's `move` is given to the body, and said back on
/// `body/gesture`.
pub const MOVE_NAME: &str = "control";

/// `look` at `name`: out of this release, the lamp having no direction of a
/// voice.
pub const NOT_IN_THIS_RELEASE: &str =
    "not in this release: look at who said the lamp's name (the lamp has no sound direction)";

/// The lamp's parts, as `lamp-control` reaches them.
pub struct Lamp {
    pub body: Option<PathBuf>,
    pub light: Option<Box<dyn LightOut>>,
    pub say: Option<PathBuf>,
    /// The boot's id, which a light request's clock names.
    pub clock_id: String,
    pub events: Events,
    /// Where a `look` goes: the body's lease socket, through the door's
    /// hold of it (`--lease-socket`).
    looks: Option<crate::body_lease::Looks>,
    /// Each client's light still asked for, to give back at its `stop`.
    lights: HashMap<String, (Light, u64)>,
    /// What the bus says, for `get` (docs/control.md, "Events").
    watch: Option<Arc<std::sync::Mutex<crate::watch::Watch>>>,
    datagram: Option<UnixDatagram>,
    /// The boot's monotonic clock (`CLOCK_MONOTONIC`), the one a claim's
    /// `until_us` is read against: not the door's own, which starts with
    /// lamp-control (the Reviewer-lead's P2 on #3805).
    clock: fn() -> u64,
    /// Whether the body follows the hand in view while the lease holds,
    /// told to it on the lease socket (`crate::body_lease`); none without
    /// one.
    follow: Option<crate::body_lease::Follower>,
    /// lamp-actions' call socket (`--actions-socket`), where the speaker's
    /// volume is the person's own: the same as the voice and the app set.
    actions: Option<PathBuf>,
    /// The camera, read from the eyes' newest frame (`--sight-file`); none
    /// without one.
    camera: Option<crate::camera::Camera>,
    /// Where the computers' own clips are kept (`crate::sounds`), which the
    /// voice plays from; none, and `sound` is the hum only.
    clips: Option<PathBuf>,
    /// Each computer's turns and mute on the voice, shared with the threads
    /// that wait on the say socket.
    voice: Arc<Mutex<Voice>>,
    /// Each client's says still waiting on the voice, for its `stop`.
    says: Says,
    /// The number the next say is kept under.
    next_say: u64,
    /// The microphone, the ears' blocks on the bus; none without the bus.
    microphone: Option<crate::microphone::Microphone>,
    /// Each computer's motion handed to the body and not yet told, when
    /// the body's `body/gesture` is read (#4209, `crate::motion_done`).
    motions: Option<Arc<Mutex<crate::motion_done::Motions>>>,
    /// The connection the request in hand came on (`Parts::asking`), for a
    /// motion's `done`.
    asking: Option<u64>,
}

/// What each computer has going on the voice.
#[derive(Default)]
struct Voice {
    /// Each computer's turn starts not yet answered.
    turns: HashMap<String, Vec<Turn>>,
    /// Each computer's latest begun turn: the voice's id for it and the
    /// request that started it. Kept past its outcome, since the voice
    /// holds the conversation a turn grew until quiet and may still end it
    /// by this id; replaced only by a newer turn's `begun`, forgotten once
    /// a `turn.end` used it. A stale id is harmless: the voice refuses it.
    begun: HashMap<String, Begun>,
    mutes: HashMap<String, Mute>,
    /// Numbers each turn and mute, so a thread that ends one never ends a
    /// newer one in its place.
    last: u64,
}

impl Voice {
    fn next(&mut self) -> u64 {
        self.last += 1;
        self.last
    }

    /// `client`'s turn start numbered `seq`, while not yet answered.
    fn turn(&mut self, client: &str, seq: u64) -> Option<&mut Turn> {
        self.turns
            .get_mut(client)?
            .iter_mut()
            .find(|turn| turn.seq == seq)
    }

    /// Forgets `client`'s mute numbered `seq`: whether it was still held.
    fn forget_mute(&mut self, client: &str, seq: u64) -> bool {
        let ours = self.mutes.get(client).is_some_and(|mute| mute.seq == seq);
        if ours {
            self.mutes.remove(client);
        }
        ours
    }
}

/// Only an explicit voice outcome is Ok; local transport loss stays Err.
type SayReceipt = Result<(&'static str, Option<String>), String>;

/// A computer's turn start not yet answered.
struct Turn {
    seq: u64,
    /// Its say socket connection while the voice has not begun it: shut
    /// when the computer goes (`gone`), which drops it at the
    /// voice, so a computer gone or revoked gets no cue and no window.
    /// Let go once it began: a begun turn is ended by its `turn.end` or
    /// the person, as before.
    waiting: Option<UnixStream>,
    /// The request that started it, for `turn.end`'s answer.
    re: u64,
    /// Ended by its own computer's `turn.end` before the door saw it begin:
    /// cancelled at the voice on its own connection, which the voice
    /// answers whether it still waited, said its prompt or had just opened
    /// its window.
    ended: bool,
    /// The `turn.end`s waiting for its final answer, told it when it comes.
    told: Vec<mpsc::Sender<SayReceipt>>,
    /// The voice said `begun`: from then on a `turn.end` ends it by its id.
    begun: bool,
}

/// A computer's begun turn: the voice's id for it, and the request that
/// started it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Begun {
    id: u64,
    re: u64,
}

/// A computer's mute: the say socket's connection holding it, once the
/// voice has taken it.
struct Mute {
    seq: u64,
    held: Option<UnixStream>,
}

impl Lamp {
    pub fn new(
        body: Option<PathBuf>,
        light: Option<Box<dyn LightOut>>,
        say: Option<PathBuf>,
        clock_id: String,
        events: Events,
    ) -> Self {
        Self {
            body,
            light,
            say,
            clock_id,
            events,
            lights: HashMap::new(),
            looks: None,
            watch: None,
            // Never blocking under the door's lock: a full mood queue is
            // the body busy, said (the Reviewer-lead's P3 on #3805).
            datagram: UnixDatagram::unbound()
                .and_then(|socket| socket.set_nonblocking(true).map(|()| socket))
                .ok(),
            clock: lamp_ipc::monotonic_us,
            follow: None,
            camera: None,
            clips: None,
            voice: Arc::default(),
            actions: None,
            says: Says::default(),
            next_say: 0,
            microphone: None,
            motions: None,
            asking: None,
        }
    }

    /// Each `gesture`, `move` and feeling's move told with a `done` once
    /// the body says on `body/gesture` (read through `gestures`) what
    /// became of it: `finished`, `cut_short` or `dropped` with the body's
    /// why (#4209). A thread of its own reads it and checks every second
    /// for a motion the body never began or never said the end of.
    pub fn telling_motions(mut self, mut gestures: lamp_bus::Subscriber) -> Self {
        let motions: Arc<Mutex<crate::motion_done::Motions>> = Arc::default();
        let kept = motions.clone();
        let events = self.events.clone();
        let clock = self.clock;
        std::thread::spawn(move || {
            loop {
                let told = match gestures.recv(std::time::Duration::from_secs(1)) {
                    Ok(Some(message)) => match message.json() {
                        Ok(payload) => kept.lock().expect("motions").said(&payload, clock()),
                        Err(_) => continue,
                    },
                    Ok(None) => kept.lock().expect("motions").lapse(clock()),
                    Err(_) => {
                        std::thread::sleep(std::time::Duration::from_millis(200));
                        kept.lock().expect("motions").lapse(clock())
                    }
                };
                for (client, done) in told {
                    events(&client, done);
                }
            }
        });
        self.motions = Some(motions);
        self
    }

    /// `client`'s request `re`, a motion named `name`, sent to the body
    /// with `send`: kept for its `done` **before** it is sent, so the
    /// body's `began` can never come first and be missed (Second's on
    /// #5835), and taken back if the body didn't take it.
    fn motion_sending(
        &mut self,
        client: &str,
        re: u64,
        name: &str,
        send: impl FnOnce(&mut Self) -> Result<Value, Error>,
    ) -> Result<Value, Error> {
        if let Some(motions) = &self.motions {
            let told = motions.lock().expect("motions").sent(
                client,
                self.asking,
                re,
                name,
                (self.clock)(),
            );
            for (client, done) in told {
                (self.events)(&client, done);
            }
        }
        let sent = send(self);
        if sent.is_err()
            && let Some(motions) = &self.motions
        {
            motions.lock().expect("motions").withdraw(client, re);
        }
        sent
    }

    /// The computers' own clips, kept under `folder` (the door's
    /// `with_sound_folder`).
    pub fn with_clips(mut self, folder: PathBuf) -> Self {
        self.clips = Some(folder);
        self
    }

    /// `camera.snapshot` and `camera.stream` from the eyes' sight file,
    /// never while `watch` says the lamp is private: after [`Self::watching`],
    /// whose watch it reads. Without a watch nothing tells the door the lamp
    /// is private, so the camera is refused.
    /// The camera's fast stream from the eyes' frames socket, its frames
    /// sent through `sink` once the server has set it.
    pub fn fast_frames(mut self, socket: PathBuf, sink: crate::camera::Sink) -> Self {
        self.camera = self.camera.map(|camera| camera.fast(socket, sink));
        self
    }

    /// A camera that opens when asked (`--camera-opens-on-demand`).
    pub fn camera_on_demand(mut self, yes: bool) -> Self {
        self.camera = self.camera.map(|camera| camera.on_demand(yes));
        self
    }

    pub fn seeing(
        mut self,
        sight: PathBuf,
        (mark, drawn): (crate::camera::Mark, crate::camera::Drawn),
    ) -> Self {
        let watch = self.watch.clone();
        let private: crate::camera::Private = Arc::new(move || {
            watch
                .as_ref()
                .is_none_or(|watch| watch.lock().map_or(true, |watch| watch.private()))
        });
        self.camera = Some(crate::camera::Camera::new(sight, private).marking(mark, drawn));
        self
    }

    /// The `volume` op through lamp-actions' call socket at `path`.
    pub fn volume_through(mut self, path: PathBuf) -> Self {
        self.actions = Some(path);
        self
    }

    /// One `volume` op: lamp-actions' app calls (`volume_to`, `volume_step`,
    /// `volume_muted`), which answer in numbers and change the volume as the
    /// voice and the app do, ceiling and floor included. The answer is the
    /// level it left, `{level, muted}`; `volume.changed` follows from
    /// `speaker/volume`. A few tens of milliseconds on a lamp (one `amixer`
    /// write and its readback), bounded by [`ACTIONS_WAIT`].
    fn volume(
        &self,
        to: Option<u8>,
        step: Option<VolumeStep>,
        mute: Option<bool>,
    ) -> Result<Value, Error> {
        let Some(path) = self.actions.as_ref() else {
            return Err(not_here("the speaker's volume"));
        };
        let (name, args) = match (to, step, mute) {
            (Some(to), _, _) => ("volume_to", json!({"volume": to})),
            (_, Some(VolumeStep::Up), _) => ("volume_step", json!({"step": "up"})),
            (_, Some(VolumeStep::Down), _) => ("volume_step", json!({"step": "down"})),
            (_, _, Some(muted)) => ("volume_muted", json!({"muted": muted})),
            _ => return Err(Error::new(ErrorCode::BadRequest, "one of to, step or mute")),
        };
        let answer = actions_call(path, name, args).map_err(|why| {
            Error::new(ErrorCode::Refused, format!("the speaker's volume: {why}"))
        })?;
        volume_of(&answer).ok_or_else(|| {
            Error::new(
                ErrorCode::Refused,
                "the speaker's volume: an answer it can't read",
            )
        })
    }

    /// `microphone.stream` from the ears' blocks that `listen` opens, its
    /// blocks sent through `sink`, only when `switched` on (the owner's
    /// `--microphone on`), and never while `watch` says the lamp is private,
    /// more than one person is in view, or the room is unseen (no fresh
    /// face count): after [`Self::watching`], as [`Self::seeing`]. Without a
    /// watch the lamp is taken as private and the room as unseen.
    pub fn hearing(
        mut self,
        listen: crate::microphone::Listen,
        (mark, drawn): (crate::camera::Mark, crate::camera::Drawn),
        sink: crate::camera::Sink,
        switched: bool,
    ) -> Self {
        let read = |says: fn(&crate::watch::Watch) -> bool| -> crate::camera::Private {
            let watch = self.watch.clone();
            Arc::new(move || {
                watch
                    .as_ref()
                    .is_none_or(|watch| watch.lock().map_or(true, |watch| says(&watch)))
            })
        };
        let private = read(|watch| watch.private());
        let watch = self.watch.clone();
        let room: crate::microphone::Room = Arc::new(move || {
            watch
                .as_ref()
                .and_then(|watch| watch.lock().ok().and_then(|watch| watch.room()))
        });
        self.microphone = Some(
            crate::microphone::Microphone::new(listen, private, sink)
                .switched(switched)
                .room(room)
                .marking(mark, drawn),
        );
        self
    }

    /// Another clock for claims, for tests.
    pub fn with_clock(mut self, clock: fn() -> u64) -> Self {
        self.clock = clock;
        self
    }

    /// `look` at `hand` follows it, through the body's lease socket.
    pub fn following(mut self, follow: crate::body_lease::Follower) -> Self {
        self.follow = Some(follow);
        self
    }

    /// Sends a `look` to the body's lease socket through the door's hold.
    pub fn looking(mut self, looks: crate::body_lease::Looks) -> Self {
        self.looks = Some(looks);
        self
    }

    /// Answers `get` from what `watch` keeps of the bus.
    pub fn watching(mut self, watch: Arc<std::sync::Mutex<crate::watch::Watch>>) -> Self {
        self.watch = Some(watch);
        self
    }

    fn to_body(&self, mut ask: Value, client: &str) -> Result<Value, Error> {
        let (Some(path), Some(socket)) = (&self.body, &self.datagram) else {
            return Err(not_here("the body"));
        };
        ask["source"] = json!(source(client));
        ask["priority"] = json!(Priority::Skill);
        socket
            .send_to(ask.to_string().as_bytes(), path)
            .map_err(|error| {
                let why = if error.kind() == std::io::ErrorKind::WouldBlock {
                    "the body is busy: its queue is full".to_owned()
                } else {
                    format!("the body did not take it: {error}")
                };
                Error::new(ErrorCode::Refused, why)
            })?;
        Ok(json!({"to": "body"}))
    }

    /// A computer's own colour or pixels (`rgb`, `pixels`), at `brightness`
    /// (0 to 100 of the ring's own ceiling): drawn as given at rest, under
    /// the same claim and priority as a named light, with the nearest named
    /// colour and level beside them for a ring that draws no pixels.
    fn own_light(
        &mut self,
        client: &str,
        given: Vec<[u8; 3]>,
        brightness: u8,
        fade_ms: Option<u32>,
        for_ms: u32,
    ) -> Result<Value, Error> {
        let share = f32::from(brightness.min(100)) / 100.0;
        let pixels: Vec<[u8; 3]> = given
            .iter()
            .map(|pixel| pixel.map(|channel| (f32::from(channel) * share).round() as u8))
            .collect();
        let light = nearest_light(&pixels);
        let until_us = (self.clock)() + u64::from(for_ms) * 1_000;
        self.publish_ring(client, light.clone(), None, fade_ms, Some(pixels), until_us)?;
        self.lights.insert(client.to_owned(), (light, until_us));
        Ok(json!({"to": "ring", "pixels": given.len()}))
    }

    fn light(
        &mut self,
        client: &str,
        colour: &str,
        level: u8,
        pattern: Option<Pattern>,
        fade_ms: Option<u32>,
        for_ms: u32,
    ) -> Result<Value, Error> {
        let pattern = pattern.map(|pattern| {
            let kind = match pattern {
                Pattern::Breathe => PatternKind::Breathe,
                Pattern::Candle => PatternKind::Candle,
                Pattern::Pulse => PatternKind::Pulse,
            };
            LightPattern {
                kind,
                period_ms: LightPattern::default_ms(kind),
                low: 1,
            }
        });
        if let Some(pattern) = &pattern {
            pattern
                .check(level)
                .map_err(|invalid| Error::new(ErrorCode::Refused, invalid.0))?;
        }
        let light = Light {
            on: true,
            colour: colour.to_owned(),
            level,
        };
        let until_us = (self.clock)() + u64::from(for_ms) * 1_000;
        self.publish_ring(client, light.clone(), pattern, fade_ms, None, until_us)?;
        self.lights.insert(client.to_owned(), (light, until_us));
        Ok(json!({"to": "ring"}))
    }

    /// Sends a motion to the body marked to replace the one playing: the
    /// body ends that and eases out of it, unless the person is using the
    /// lamp, which it refuses with why.
    fn motion(&mut self, mut ask: Value, client: &str) -> Result<Value, Error> {
        ask["replace"] = json!(true);
        self.to_body(ask, client)
    }

    fn express(
        &mut self,
        client: &str,
        re: u64,
        feeling: &str,
        strength: f32,
    ) -> Result<Value, Error> {
        let feeling =
            feelings::named(feeling).map_err(|why| Error::new(ErrorCode::BadRequest, why))?;
        let mut answer = json!({"to": "body"});
        match feeling
            .scaled(strength)
            .map_err(|why| Error::new(ErrorCode::Refused, why))?
        {
            Some(scaled) => {
                answer["move"] = json!(scaled.name);
                let name = scaled.name.clone();
                self.motion_sending(client, re, &name, |lamp| {
                    lamp.motion(json!({"move": scaled}), client)
                })?;
            }
            None => answer["move"] = Value::Null,
        }
        let now = self.state(StateKind::Light);
        match feeling.light(&now) {
            Ok((light, pattern)) => {
                let until_us = (self.clock)() + u64::from(feeling.lasting_ms) * 1_000;
                match self.ask_ring(client, light.clone(), pattern, until_us) {
                    Ok(()) => {
                        self.lights.insert(client.to_owned(), (light, until_us));
                        answer["light"] = json!(true);
                    }
                    Err(error) => answer["light"] = json!(error.why),
                }
            }
            Err(why) => answer["light"] = json!(why),
        }
        Ok(answer)
    }

    fn look(&self, client: &str, re: u64, at: &LookAt) -> Result<Value, Error> {
        // A look at `hand` starts the body following it (said on each hold);
        // any other look ends that once it is taken, never a refused one
        // (Nine's P3 on #4375: the SDKs end a following only on a taken look).
        let look = match at {
            LookAt::Target(LookTarget::Hand) => {
                let Some(follow) = &self.follow else {
                    return Err(waits_for("the body's lease socket"));
                };
                *follow.lock().expect("follower") = Some(client.to_owned());
                return Ok(json!({"to": "body", "follow": "hand"}));
            }
            LookAt::Angles(Angles { yaw, pitch }) => json!({"yaw": yaw, "pitch": pitch}),
            LookAt::Target(LookTarget::Ahead) => json!("ahead"),
            // One turn toward the face the eyes last showed, said by the body.
            LookAt::Target(LookTarget::Face) => json!("face"),
            // Out of this release, said as that (the local leader, 29 Oct).
            LookAt::Target(LookTarget::Name) => {
                return Err(Error::new(ErrorCode::Refused, NOT_IN_THIS_RELEASE));
            }
        };
        let Some(looks) = &self.looks else {
            return Err(Error::new(
                ErrorCode::Refused,
                "the body's lease socket is not on here",
            ));
        };
        looks
            .send(crate::body_lease::Look {
                client: client.to_owned(),
                re,
                line: json!({ "look": look }).to_string(),
                join: None,
            })
            .map_err(|_| not_here("the body's lease"))?;
        if let Some(follow) = &self.follow {
            *follow.lock().expect("follower") = None;
        }
        Ok(json!({"to": "body"}))
    }

    /// `joints`: the head's `yaw` and `pitch`, in degrees from home, go as
    /// the holder's look on the body's lease socket, said `"from":"home"`:
    /// the body takes its pose's offset off, so a standing lamp's stand pose
    /// doesn't move the frame (its `done` says where the head got to, from
    /// home too). The head's `roll` and the arm's `shoulder` and `elbow` go
    /// as the holder's pose on the same socket (`{"pose":…}`, lamp-body's
    /// `Posing`), in degrees as `get` `body` counts them: the roll from its
    /// found home, the arm from the folded arm; the body takes a pose only
    /// while it stands, inside the arm's window, and says `refused` with why
    /// otherwise. A `joints` naming both sends both lines, and its one `done`
    /// comes when both are answered ([`crate::body_lease::Join`]).
    ///
    /// When `yaw` or `pitch` is named, the other one not named stays where
    /// it is now, as `get` `body` says it; with no position heard in the last
    /// 15 s (the body stopped or restarted) it is refused, never sent from an
    /// old one. An arm joint not named stays where the pose has it.
    fn joints(
        &self,
        client: &str,
        re: u64,
        targets: &crate::protocol::Joints,
    ) -> Result<Value, Error> {
        let head = targets.yaw.is_some() || targets.pitch.is_some();
        let arm = targets.roll.is_some() || targets.shoulder.is_some() || targets.elbow.is_some();
        let mut lines = Vec::new();
        if head {
            let now = |joint: &str| {
                self.watch
                    .as_ref()
                    .and_then(|watch| watch.lock().ok().map(|watch| watch.state(StateKind::Body)))
                    .and_then(|body| body["joints"][joint]["deg"].as_f64())
                    .map(|deg| deg as f32)
            };
            let (Some(yaw), Some(pitch)) = (
                targets.yaw.or_else(|| now("yaw")),
                targets.pitch.or_else(|| now("pitch")),
            ) else {
                return Err(Error::new(
                    ErrorCode::Refused,
                    "name both yaw and pitch while the body has not said where it is in the last 15 s (get body)",
                ));
            };
            lines.push(json!({"look": {"yaw": yaw, "pitch": pitch, "from": "home"}}).to_string());
        }
        if arm {
            let mut pose = serde_json::Map::new();
            for (name, degrees) in [
                ("roll", targets.roll),
                ("shoulder", targets.shoulder),
                ("elbow", targets.elbow),
            ] {
                if let Some(degrees) = degrees {
                    pose.insert(name.to_owned(), json!(degrees));
                }
            }
            lines.push(json!({ "pose": pose }).to_string());
        }
        let Some(looks) = &self.looks else {
            return Err(Error::new(
                ErrorCode::Refused,
                "the body's lease socket is not on here",
            ));
        };
        let join = (lines.len() > 1).then(crate::body_lease::Join::default);
        for line in lines {
            looks
                .send(crate::body_lease::Look {
                    client: client.to_owned(),
                    re,
                    line,
                    join: join.clone(),
                })
                .map_err(|_| not_here("the body's lease"))?;
        }
        // As any look taken: a hand-follow ends.
        if head && let Some(follow) = &self.follow {
            *follow.lock().expect("follower") = None;
        }
        Ok(json!({"to": "body"}))
    }

    fn ask_ring(
        &mut self,
        client: &str,
        light: Light,
        pattern: Option<LightPattern>,
        until_us: u64,
    ) -> Result<(), Error> {
        self.publish_ring(client, light, pattern, None, None, until_us)
    }

    fn publish_ring(
        &mut self,
        client: &str,
        light: Light,
        pattern: Option<LightPattern>,
        fade_ms: Option<u32>,
        pixels: Option<Vec<[u8; 3]>>,
        until_us: u64,
    ) -> Result<(), Error> {
        let Some(out) = self.light.as_mut() else {
            return Err(not_here("the ring"));
        };
        let request = LightRequest {
            claim: Claim {
                source: source(client),
                priority: Priority::Skill,
                clock_id: self.clock_id.clone(),
                until_us,
            },
            light,
            pattern,
            fade_ms,
            pixels,
        };
        match out.publish(&request) {
            Ok(0) => Err(Error::new(
                ErrorCode::Refused,
                "the ring took no light request (LAMP_LIGHT_REQUESTS off?)",
            )),
            Ok(_) => Ok(()),
            Err(error) => Err(Error::new(
                ErrorCode::Refused,
                format!("the ring did not take it: {error}"),
            )),
        }
    }

    /// `say`: the computer's words exactly as sent, rendered straight
    /// through the voice's speech service with no model or tools (`exact`),
    /// spoken once the person isn't talking; with `rephrase`, given to the
    /// voice's model as a written turn, as before.
    fn say(&mut self, client: &str, re: u64, text: &str, rephrase: bool) -> Result<Value, Error> {
        let mut line = json!({"skill": "control", "text": text, "expires_in_ms": SAY_EXPIRES_MS});
        if !rephrase {
            line["exact"] = json!(true);
        }
        self.say_to_voice(client, re, line)
    }

    /// `sound`: the lamp's own short hum, the voice's built-in cue (no file,
    /// no model), through the same queue, quiet, rarity and privacy gates as
    /// a skill's sound; or one of the computer's own clips by name, played
    /// from the clips folder as an exact line is (once the person isn't
    /// talking, no rarity). Its outcome is a `done`, as a say's is.
    fn sound(&mut self, client: &str, re: u64, name: &str) -> Result<Value, Error> {
        if crate::protocol::SOUNDS.contains(&name) {
            return self.say_to_voice(
                client,
                re,
                json!({"skill": "control", "cue": "hum", "expires_in_ms": SAY_EXPIRES_MS}),
            );
        }
        let Some(folder) = &self.clips else {
            return Err(Error::new(ErrorCode::Refused, crate::session::SOUNDS_OFF));
        };
        let Some(path) =
            crate::sounds::Sounds::path(folder, client, name).filter(|path| path.is_file())
        else {
            return Err(Error::new(
                ErrorCode::Refused,
                format!(
                    "no clip named {name} on this lamp for this computer: sound.upload it first"
                ),
            ));
        };
        self.say_to_voice(
            client,
            re,
            json!({"skill": "control", "clip": path, "expires_in_ms": SAY_EXPIRES_MS}),
        )
    }

    /// `turn.start`: a turn asked of the voice, as a say is, its outcome a
    /// `done`. The turn's id from the voice's `begun` is kept for this
    /// computer's `turn.end`, in place of its earlier turn's; a start
    /// refused before its `begun` leaves the earlier id. The earlier id
    /// goes with the start as `after`: the voice opens a turn of the same
    /// computer's during that turn's own conversation, and refuses it only
    /// for the person's conversation or another computer's turn.
    fn turn_start(&self, client: &str, re: u64, prompt: Option<&str>) -> Result<Value, Error> {
        let Some(path) = self.say.clone() else {
            return Err(not_here("the voice"));
        };
        let mut line =
            json!({"skill": "control", "turn": "start", "expires_in_ms": SAY_EXPIRES_MS});
        if let Some(prompt) = prompt {
            line["text"] = json!(prompt);
        }
        let seq = {
            let mut voice = self.voice.lock().expect("voice");
            if let Some(earlier) = voice.begun.get(client) {
                line["after"] = json!(earlier.id);
            }
            let seq = voice.next();
            voice
                .turns
                .entry(client.to_owned())
                .or_default()
                .push(Turn {
                    seq,
                    waiting: None,
                    re,
                    ended: false,
                    begun: false,
                    told: Vec::new(),
                });
            seq
        };
        let (voice, events, client) = (self.voice.clone(), self.events.clone(), client.to_owned());
        // Off the door's lock, as a say.
        std::thread::spawn(move || {
            // Its id replaces the earlier turn's only now, as it begins.
            let begun = |said: &Value| {
                if let Some(id) = said["id"].as_u64() {
                    let mut voice = voice.lock().expect("voice");
                    voice.begun.insert(client.clone(), Begun { id, re });
                    if let Some(turn) = voice.turn(&client, seq) {
                        turn.waiting = None;
                        turn.begun = true;
                    }
                }
            };
            let receipt = match UnixStream::connect(&path)
                .and_then(|mut stream| writeln!(stream, "{line}").map(|()| stream))
            {
                Ok(stream) => {
                    // Kept with the start while it waits; gone meanwhile, its
                    // computer's start is let go at once.
                    let kept = stream.try_clone().ok();
                    // Ended by its own `turn.end` before it connected:
                    // cancelled at once on its connection, and its answer
                    // read as any other's.
                    let waits = match voice.lock().expect("voice").turn(&client, seq) {
                        Some(turn) if !turn.ended => {
                            turn.waiting = kept;
                            true
                        }
                        _ => false,
                    };
                    if !waits {
                        cancel_turn_before(&stream, Instant::now() + SAY_CANCEL_WITHIN);
                    }
                    let _ = stream.set_read_timeout(Some(SAY_ANSWER_WITHIN));
                    outcome_with(BufReader::new(stream), begun)
                }
                Err(error) => Err(format!("the voice did not take it: {error}")),
            };
            let (how, why, answered) = match receipt {
                Ok((how, why)) => (how, why, true),
                Err(why) => ("dropped", Some(why), false),
            };
            // A voice from before turns refuses the field it doesn't know.
            let why = why.map(|why| {
                if why == format!("refused: {NOT_ITS_PROTOCOL}") {
                    format!("refused: {NO_TURNS}")
                } else {
                    why
                }
            });
            // Answered: no longer a start waiting. Its id, if it began,
            // stays for a `turn.end` while its conversation may go on.
            let (ended, told) = {
                let mut voice = voice.lock().expect("voice");
                let (ended, told) = match voice.turn(&client, seq) {
                    Some(turn) => (turn.ended, std::mem::take(&mut turn.told)),
                    None => (false, Vec::new()),
                };
                if let Some(turns) = voice.turns.get_mut(&client) {
                    turns.retain(|turn| turn.seq != seq);
                    if turns.is_empty() {
                        voice.turns.remove(&client);
                    }
                }
                (ended, told)
            };
            // Cancelled by its own computer while the voice still had it
            // waiting: said so in the door's words. Once the voice had
            // taken it (its prompt said, its window opened), the voice's.
            let (how, why) = if ended
                && how == "dropped"
                && why
                    .as_deref()
                    .is_some_and(|why| why.ends_with(STOPPED_BY_ASKER))
            {
                ("dropped", Some(ENDED_BEFORE_BEGUN.to_owned()))
            } else {
                (how, why)
            };
            let receipt = if answered {
                Ok((how, why.clone()))
            } else {
                Err(why
                    .clone()
                    .unwrap_or_else(|| "the voice gave no outcome".into()))
            };
            for tell in told {
                let _ = tell.send(receipt.clone());
            }
            let mut done = json!({"ev": "done", "re": re, "how": how});
            if let Some(why) = why {
                done["why"] = json!(why);
            }
            events(&client, done);
        });
        Ok(json!({"to": "voice"}))
    }

    /// `turn.end`: the end of this computer's own turn, by the id the voice
    /// gave its latest begun turn, answered once the voice says. Without a
    /// turn of its own the voice isn't asked. The id is forgotten once the
    /// voice ended it or said it is no turn of this computer's.
    fn turn_end(&self, client: &str) -> Result<Reply, Error> {
        let Some(path) = self.say.clone() else {
            return Err(not_here("the voice"));
        };
        let cancel_by = Instant::now() + TURN_CANCEL_WITHIN;
        let begun = {
            let mut voice = self.voice.lock().expect("voice");
            // Mark this computer's pending starts for cancellation under the
            // state lock; their own connections are cancelled off that lock.
            // Only the voice can confirm what phase each one reached.
            // Checked first: an earlier turn's id, kept past its
            // end, must not take its place (lamp-75c9, 9 Nov: `turn.end` 2 s
            // after a `turn.start` was refused "not this computer's turn").
            if let Some(turns) = voice.turns.get_mut(client) {
                let mut ending = Vec::new();
                for turn in turns.iter_mut().filter(|turn| !turn.ended && !turn.begun) {
                    // Keep the original for gone/revocation. The copy writes
                    // off the door and voice locks; a not-yet-connected start
                    // observes ended and cancels itself as it connects.
                    let waiting = turn.waiting.as_ref().and_then(|stream| {
                        match stream.try_clone() {
                            Ok(copy) => Some(copy),
                            Err(_) => {
                                // EOF drops a start still queued at the voice.
                                let _ = stream.shutdown(Shutdown::Write);
                                None
                            }
                        }
                    });
                    turn.ended = true;
                    let (tell, told) = mpsc::channel();
                    turn.told.push(tell);
                    ending.push((turn.re, waiting, told));
                }
                if let Some(&(started, _, _)) = ending.last() {
                    // Answered only once the voice said what became of each:
                    // never from the door's own view, which can't tell a
                    // start still waiting from one whose prompt is said.
                    return Ok(Reply::Later(Box::new(move || {
                        for (_, waiting, _) in &ending {
                            if let Some(stream) = waiting {
                                cancel_turn_before(stream, cancel_by);
                            }
                        }
                        let timed_out = || {
                            Error::new(
                                ErrorCode::Refused,
                                format!(
                                    "the voice did not say within {} s whether it ended",
                                    TURN_CANCEL_WITHIN.as_secs()
                                ),
                            )
                        };
                        for (_, _, told) in ending {
                            let remaining = cancel_by.saturating_duration_since(Instant::now());
                            if remaining.is_zero() {
                                return Err(timed_out());
                            }
                            match told.recv_timeout(remaining) {
                                Ok(Ok(("finished", _))) => {
                                    return Err(Error::new(
                                        ErrorCode::Refused,
                                        "the turn had already finished",
                                    ));
                                }
                                Ok(Ok(_)) => {}
                                Ok(Err(why)) => return Err(Error::new(ErrorCode::Refused, why)),
                                Err(mpsc::RecvTimeoutError::Disconnected) => {
                                    return Err(Error::new(
                                        ErrorCode::Refused,
                                        "the voice gave no outcome",
                                    ));
                                }
                                Err(mpsc::RecvTimeoutError::Timeout) => return Err(timed_out()),
                            }
                        }
                        Ok(json!({"turn": "ended", "started": started}))
                    })));
                }
            }
            match voice.begun.get(client) {
                Some(begun) => *begun,
                None => return Err(Error::new(ErrorCode::Busy, NOT_THIS_TURN)),
            }
        };
        let (voice, client) = (self.voice.clone(), client.to_owned());
        Ok(Reply::Later(Box::new(move || {
            let (_, said) = ask_voice(
                &path,
                &json!({"skill": "control", "turn": "end", "id": begun.id}),
            )?;
            let ended = said["outcome"] == "ended";
            if ended || said["why"] == NOT_THIS_TURN {
                // Used: forgotten, unless a newer turn began meanwhile.
                let mut voice = voice.lock().expect("voice");
                if voice.begun.get(&client) == Some(&begun) {
                    voice.begun.remove(&client);
                }
            }
            if ended {
                Ok(json!({"turn": "ended", "started": begun.re}))
            } else {
                Err(refusal_of(&said, NO_TURNS))
            }
        })))
    }

    /// `listen.set`: `false` holds this computer's mute on one say socket
    /// connection, answered once the voice took it; again while held, the
    /// same. `true` lifts it on that connection and closes it; with none
    /// held, nothing is asked.
    fn listen(&self, client: &str, re: u64, on: bool) -> Result<Reply, Error> {
        let mut voice = self.voice.lock().expect("voice");
        match voice.mutes.get(client) {
            Some(Mute { held: None, .. }) => {
                return Err(Error::new(ErrorCode::Busy, MUTE_BEING_TAKEN));
            }
            Some(Mute { held: Some(_), .. }) if !on => {
                return Ok(Reply::Now(json!({"mute": "held"})));
            }
            Some(Mute { held: Some(_), .. }) => {
                if let Some(Mute {
                    held: Some(held), ..
                }) = voice.mutes.remove(client)
                {
                    lift(&held);
                }
                return Ok(Reply::Now(json!({"mute": "lifted"})));
            }
            None if on => return Ok(Reply::Now(json!({"mute": "nothing_held"}))),
            None => {}
        }
        let Some(path) = self.say.clone() else {
            return Err(not_here("the voice"));
        };
        let seq = voice.next();
        voice
            .mutes
            .insert(client.to_owned(), Mute { seq, held: None });
        drop(voice);
        let (voice, events, client) = (self.voice.clone(), self.events.clone(), client.to_owned());
        Ok(Reply::Later(Box::new(move || {
            let asked = ask_voice(&path, &json!({"skill": "control", "listen": false}));
            let reader = match asked {
                Ok((reader, said)) if said["outcome"] == "muted" => reader,
                Ok((_, said)) => {
                    voice.lock().expect("voice").forget_mute(&client, seq);
                    return Err(refusal_of(&said, NO_MUTE));
                }
                Err(error) => {
                    voice.lock().expect("voice").forget_mute(&client, seq);
                    return Err(error);
                }
            };
            {
                let mut held = voice.lock().expect("voice");
                let kept = reader.get_ref().try_clone();
                match (held.mutes.get_mut(&client), kept) {
                    (Some(mute), Ok(kept)) if mute.seq == seq => mute.held = Some(kept),
                    // Gone while the voice took it (its last connection, a
                    // revoke), or no way to keep it: let go at once.
                    (_, kept) => {
                        held.forget_mute(&client, seq);
                        drop(held);
                        let _ = reader.get_ref().shutdown(Shutdown::Both);
                        return Err(Error::new(
                            ErrorCode::Refused,
                            match kept {
                                Ok(_) => "this computer went as the voice took its mute".to_owned(),
                                Err(error) => {
                                    format!("the mute's connection can't be kept: {error}")
                                }
                            },
                        ));
                    }
                }
            }
            std::thread::spawn(move || watch_mute(reader, &voice, &client, seq, re, &events));
            Ok(json!({"mute": "held"}))
        })))
    }

    /// A line to the voice for `client`'s action `re`, kept for its `stop`
    /// (`{"cancel":true}` on the same connection) until its outcome comes.
    fn say_to_voice(&mut self, client: &str, re: u64, line: Value) -> Result<Value, Error> {
        let Some(path) = self.say.clone() else {
            return Err(not_here("the voice"));
        };
        let events = self.events.clone();
        let client = client.to_owned();
        // Kept before the thread starts, so that a `stop` right after the
        // reply still reaches it.
        let number = self.next_say;
        self.next_say += 1;
        let says = Arc::clone(&self.says);
        says.lock()
            .expect("says")
            .entry(client.clone())
            .or_default()
            .insert(number, None);
        // Off the door's lock, connecting included (the Reviewer-lead's P3
        // on #3805: a voice whose backlog is full must not hold every
        // connection). The outcome comes when the turn ends, as `done`; a
        // voice that can't be reached is a `done` dropped, with why.
        std::thread::spawn(move || {
            let (how, why) = match ask_voice_for_say(&path, &line, &says, &client, number) {
                Ok(Some(stream)) => {
                    let _ = stream.set_read_timeout(Some(SAY_ANSWER_WITHIN));
                    outcome(BufReader::new(stream))
                }
                Ok(None) => ("dropped", Some(format!("dropped: {STOPPED_BY_ASKER}"))),
                Err(error) => (
                    "dropped",
                    Some(format!("the voice did not take it: {error}")),
                ),
            };
            forget_say(&says, &client, number);
            let mut done = json!({"ev": "done", "re": re, "how": how});
            if let Some(why) = why {
                done["why"] = json!(why);
            }
            events(&client, done);
        });
        Ok(json!({"to": "voice"}))
    }
}

/// The voice's why for a computer's turn ended by the person beginning to
/// talk with the lamp in its window (`lamp_voice::control_turn::PERSON_TALKING`).
const PERSON_TALKING: &str = "the person is talking";
/// The voice's whys a turn's `done` gives as `cut_short`, not `dropped`:
/// the person began talking in its window, or its own computer's
/// `turn.end` cut its answer (`lamp_voice::control_turn::CUT_BY_ITS_COMPUTER`).
const CUT_SHORT_WHYS: [&str; 2] = [PERSON_TALKING, "ended by this computer"];

/// Asks the voice for say `number` of `client`, unless its `stop` came
/// first (`None`): the connection the outcome comes on, kept for a `stop`;
/// one that came while connecting is sent on at once.
fn ask_voice_for_say(
    path: &std::path::Path,
    line: &Value,
    says: &Says,
    client: &str,
    number: u64,
) -> std::io::Result<Option<UnixStream>> {
    if !is_kept(says, client, number) {
        return Ok(None);
    }
    let mut stream = UnixStream::connect(path)?;
    writeln!(stream, "{line}")?;
    let _ = stream.set_write_timeout(Some(SAY_CANCEL_WITHIN));
    let kept = stream.try_clone()?;
    let mut says = says.lock().expect("says");
    match says
        .get_mut(client)
        .and_then(|asked| asked.get_mut(&number))
    {
        Some(slot) => *slot = Some(kept),
        None => {
            let _ = writeln!(&kept, "{SAY_CANCEL}");
        }
    }
    Ok(Some(stream))
}

fn is_kept(says: &Says, client: &str, number: u64) -> bool {
    says.lock()
        .expect("says")
        .get(client)
        .is_some_and(|asked| asked.contains_key(&number))
}

/// Say `number` of `client` is answered: no `stop` reaches it now.
fn forget_say(says: &Says, client: &str, number: u64) {
    let mut says = says.lock().expect("says");
    if let Some(asked) = says.get_mut(client) {
        asked.remove(&number);
        if asked.is_empty() {
            says.remove(client);
        }
    }
}

/// Attempt every cancellation within the shared budget. A blocked or failed
/// write half-closes the request side: the voice drops a start still queued,
/// but EOF does not cancel one already begun. Its actual outcome can still
/// arrive on the read side; EOF alone never confirms the end.
fn cancel_turn_before(mut stream: &UnixStream, deadline: Instant) {
    let sent = (|| -> std::io::Result<()> {
        for mut bytes in [SAY_CANCEL.as_bytes(), b"\n"] {
            while !bytes.is_empty() {
                // Partial writes and interruptions cannot renew the budget.
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(std::io::ErrorKind::TimedOut.into());
                }
                stream.set_write_timeout(Some(remaining.min(SAY_CANCEL_WITHIN)))?;
                let wrote = match stream.write(bytes) {
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    result => result?,
                };
                if wrote == 0 {
                    return Err(std::io::ErrorKind::WriteZero.into());
                }
                bytes = &bytes[wrote..];
            }
        }
        Ok(())
    })();
    if sent.is_err() {
        let _ = stream.shutdown(Shutdown::Write);
    }
}

/// The say socket's outcome, past the `begun` event, in the protocol's
/// words (`done.how`: `finished`, `cut_short`, `dropped`; the Reviewer-lead's
/// and harness-reviewer's P2 on #3805): `spoken` is `finished`; `dropped`,
/// `expired`, `refused` and no outcome at all are `dropped`, the voice's
/// own word and why in `why`.
fn outcome(reader: impl std::io::BufRead) -> (&'static str, Option<String>) {
    outcome_with(reader, |_| {}).unwrap_or_else(|why| ("dropped", Some(why)))
}

/// As [`outcome`], each event before it (`begun`) handed to `event`.
/// Missing or unexpected outcomes remain transport/protocol uncertainty.
fn outcome_with(reader: impl std::io::BufRead, mut event: impl FnMut(&Value)) -> SayReceipt {
    for line in reader.lines() {
        let Ok(line) = line else { break };
        let Ok(said) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if said.get("event").is_some() {
            event(&said);
            continue;
        }
        let Some(word) = said["outcome"].as_str() else {
            continue;
        };
        if !matches!(word, "spoken" | "dropped" | "expired" | "refused") {
            return Err("the voice gave an unexpected say outcome".into());
        }
        if word == "spoken" {
            return Ok(("finished", None));
        }
        // A computer's turn ended because the person began talking with
        // the lamp in its window: the person wins, and the turn was cut
        // short, not dropped.
        // So is one whose answer its own computer cut with `turn.end`.
        if let Some(why) = said["why"]
            .as_str()
            .filter(|why| CUT_SHORT_WHYS.contains(why))
            && word == "dropped"
        {
            return Ok(("cut_short", Some(why.to_owned())));
        }
        let why = match said["why"].as_str() {
            Some(why) => format!("{word}: {why}"),
            None => word.to_owned(),
        };
        return Ok(("dropped", Some(why)));
    }
    Err("the voice gave no outcome".to_owned())
}

/// `line` on a new say socket connection, and the voice's first outcome on
/// it (past any event): the connection kept, to read on or hold.
fn ask_voice(
    path: &std::path::Path,
    line: &Value,
) -> Result<(BufReader<UnixStream>, Value), Error> {
    let not_taken = |error: std::io::Error| {
        Error::new(
            ErrorCode::Refused,
            format!("the voice did not take it: {error}"),
        )
    };
    let mut stream = UnixStream::connect(path).map_err(not_taken)?;
    writeln!(stream, "{line}").map_err(not_taken)?;
    let _ = stream.set_read_timeout(Some(VOICE_ANSWER_WITHIN));
    let mut reader = BufReader::new(stream);
    let mut said = String::new();
    loop {
        said.clear();
        match reader.read_line(&mut said) {
            Ok(0) | Err(_) => {
                return Err(Error::new(ErrorCode::Refused, "the voice gave no answer"));
            }
            Ok(_) => {}
        }
        if let Ok(said) = serde_json::from_str::<Value>(&said)
            && said.get("outcome").is_some()
        {
            return Ok((reader, said));
        }
    }
}

/// The voice's refusal of a `turn.end` or a `listen`, in the protocol's
/// words: `busy` for another's turn, the voice's own why otherwise, and
/// `older` for a voice from before these, which doesn't know the field.
fn refusal_of(said: &Value, older: &str) -> Error {
    let word = said["outcome"].as_str().unwrap_or("failed");
    let why = said["why"].as_str();
    match (word, why) {
        ("refused", Some(NOT_THIS_TURN)) => Error::new(ErrorCode::Busy, NOT_THIS_TURN),
        ("refused", Some(NOT_ITS_PROTOCOL)) => Error::new(ErrorCode::Refused, older),
        ("refused", Some(why)) => Error::new(ErrorCode::Refused, why),
        (word, Some(why)) => Error::new(ErrorCode::Refused, format!("{word}: {why}")),
        (word, None) => Error::new(ErrorCode::Refused, word),
    }
}

/// Lifts a held mute: `listen: true` on its own connection, then closed,
/// which lifts it too if the line didn't reach the voice.
fn lift(held: &UnixStream) {
    let _ = held.set_write_timeout(Some(Duration::from_millis(200)));
    let mut out = held;
    let _ = writeln!(out, "{}", json!({"skill": "control", "listen": true}));
    let _ = held.shutdown(Shutdown::Both);
}

/// Reads a held mute's connection until it ends. Ended by the voice (a
/// hand on the head lifted it, or the voice let the connection go) while
/// still this computer's, it is forgotten and the computer told, a `done`
/// naming the `listen.set` that asked for it, `cut_short` with the voice's
/// why. Ended by the door (`listen.set true`, the computer gone), nothing
/// is told.
fn watch_mute(
    reader: BufReader<UnixStream>,
    voice: &Mutex<Voice>,
    client: &str,
    seq: u64,
    re: u64,
    events: &Events,
) {
    let _ = reader.get_ref().set_read_timeout(None);
    let mut why = "the voice let it go".to_owned();
    for line in reader.lines() {
        let Ok(line) = line else { break };
        if let Ok(said) = serde_json::from_str::<Value>(&line)
            && said["outcome"] == "lifted"
        {
            why = said["why"].as_str().unwrap_or("lifted").to_owned();
            break;
        }
    }
    if voice.lock().expect("voice").forget_mute(client, seq) {
        eprintln!("lamp-control: {client}'s mute was lifted by the voice: {why}");
        events(
            client,
            json!({"ev": "done", "re": re, "how": "cut_short", "why": why}),
        );
    }
}

/// The named light nearest a computer's own pixels, for a ring that draws
/// none and for `light/drawn`: the tint nearest their mean colour, at the
/// level nearest their brightness (the brightest channel of the brightest
/// pixel), off when every pixel is.
fn nearest_light(pixels: &[[u8; 3]]) -> Light {
    let brightest = pixels
        .iter()
        .flat_map(|pixel| pixel.iter().copied())
        .max()
        .unwrap_or(0);
    if brightest == 0 {
        return Light {
            on: false,
            colour: "warm".to_owned(),
            level: 1,
        };
    }
    let lit: Vec<&[u8; 3]> = pixels
        .iter()
        .filter(|pixel| pixel.iter().any(|c| *c > 0))
        .collect();
    let mean: [f32; 3] = [0, 1, 2]
        .map(|at| lit.iter().map(|pixel| f32::from(pixel[at])).sum::<f32>() / lit.len() as f32);
    let top = mean.iter().copied().fold(0.0_f32, f32::max).max(1.0);
    let tint = mean.map(|channel| channel / top);
    let colour = Light::TINTS
        .iter()
        .min_by(|(_, a), (_, b)| {
            let far = |t: &[f32; 3]| (0..3).map(|at| (t[at] - tint[at]).powi(2)).sum::<f32>();
            far(a).total_cmp(&far(b))
        })
        .map_or("white", |(name, _)| name);
    let level = ((f32::from(brightest) / 255.0 * f32::from(Light::BRIGHTEST)).round() as u8)
        .clamp(1, Light::BRIGHTEST);
    Light {
        on: true,
        colour: colour.to_owned(),
        level,
    }
}

fn source(client: &str) -> String {
    format!("control:{client}")
}

/// How long a call to lamp-actions may take, connecting, asking and the
/// answer: an `amixer` write and its readback are tens of milliseconds.
pub const ACTIONS_WAIT: Duration = Duration::from_millis(1_500);

/// One call on lamp-actions' socket, its answer when `ok`, or why not.
fn actions_call(path: &std::path::Path, name: &str, args: Value) -> Result<Value, String> {
    let gone = |_| "lamp-actions is not running".to_owned();
    let mut stream = UnixStream::connect(path).map_err(gone)?;
    stream.set_read_timeout(Some(ACTIONS_WAIT)).map_err(gone)?;
    stream.set_write_timeout(Some(ACTIONS_WAIT)).map_err(gone)?;
    writeln!(
        stream,
        "{}",
        json!({"op": "call", "name": name, "args": args})
    )
    .map_err(gone)?;
    let mut line = String::new();
    std::io::BufRead::read_line(&mut BufReader::new(stream), &mut line)
        .map_err(|_| "lamp-actions did not answer".to_owned())?;
    let answer: Value =
        serde_json::from_str(&line).map_err(|_| "lamp-actions said nonsense".to_owned())?;
    if answer["ok"] == true {
        Ok(answer)
    } else {
        Err(answer["error"].as_str().unwrap_or("not done").to_owned())
    }
}

/// `{level, muted}` from lamp-actions' `{"level": {"volume", "raw", ...}}`:
/// muted is a raw level of nothing.
fn volume_of(answer: &Value) -> Option<Value> {
    let level = answer["level"]["volume"]
        .as_u64()
        .filter(|level| *level <= 100)?;
    let raw = answer["level"]["raw"].as_u64()?;
    Some(json!({"level": level, "muted": raw == 0}))
}

fn not_here(part: &str) -> Error {
    Error::new(
        ErrorCode::Refused,
        format!("{part} is not reachable from lamp-control here"),
    )
}

fn waits_for(what: &str) -> Error {
    Error::new(
        ErrorCode::Refused,
        format!("not routed yet: waits for {what}"),
    )
}

impl Parts for Lamp {
    fn act(&mut self, client: &str, re: u64, op: &Op, _now_us: u64) -> Result<Value, Error> {
        match op {
            Op::Gesture { name } => self.motion_sending(client, re, name, |lamp| {
                lamp.motion(json!({"gesture": name}), client)
            }),
            Op::Move { step_ms, frames } => self.motion_sending(client, re, MOVE_NAME, |lamp| {
                lamp.motion(
                    json!({"move": {"name": MOVE_NAME, "step_ms": step_ms, "frames": frames}}),
                    client,
                )
            }),
            Op::Light {
                colour: Some(colour),
                level,
                pattern,
                fade_ms,
                for_ms,
                ..
            } => self.light(
                client,
                colour,
                level.unwrap_or(1),
                *pattern,
                *fade_ms,
                *for_ms,
            ),
            Op::Light {
                rgb,
                pixels,
                brightness,
                fade_ms,
                for_ms,
                ..
            } => {
                let given: Vec<[u8; 3]> = match (rgb, pixels) {
                    (Some(rgb), _) => vec![
                        crate::protocol::hex_rgb(rgb).unwrap_or_default();
                        lamp_msgs::LightRequest::PIXELS
                    ],
                    (_, Some(pixels)) => pixels
                        .iter()
                        .map(|pixel| crate::protocol::hex_rgb(pixel).unwrap_or_default())
                        .collect(),
                    _ => {
                        return Err(Error::new(
                            ErrorCode::BadRequest,
                            "one of colour, rgb or pixels",
                        ));
                    }
                };
                self.own_light(client, given, brightness.unwrap_or(100), *fade_ms, *for_ms)
            }
            Op::Say { text, rephrase } => self.say(client, re, text, rephrase.unwrap_or(false)),
            Op::Volume { to, step, mute } => self.volume(*to, *step, *mute),
            Op::Look { at } => self.look(client, re, at),
            Op::Joints { targets, .. } => self.joints(client, re, targets),
            Op::CameraSnapshot {} => self
                .camera
                .as_ref()
                .ok_or_else(|| not_here("the camera"))?
                .snapshot(client),
            Op::CameraStream { on, fps, size } => {
                let events = self.events.clone();
                self.camera
                    .as_mut()
                    .ok_or_else(|| not_here("the camera"))?
                    .stream(client, *on, *fps, size.as_deref(), events)
            }
            Op::MicrophoneStream { on } => {
                let events = self.events.clone();
                self.microphone
                    .as_mut()
                    .ok_or_else(|| not_here("the microphone"))?
                    .stream(client, *on, events)
            }
            Op::Express { feeling, strength } => self.express(client, re, feeling, *strength),
            Op::Sound { name } => self.sound(client, re, name),
            Op::Play { .. } => Err(waits_for("client packages")),
            Op::TurnStart { prompt } => self.turn_start(client, re, prompt.as_deref()),
            _ => Err(Error::new(ErrorCode::BadRequest, "not an action")),
        }
    }

    fn reply(&mut self, client: &str, re: u64, op: &Op, now_us: u64) -> Result<Reply, Error> {
        match op {
            Op::TurnEnd {} => self.turn_end(client),
            Op::ListenSet { on } => self.listen(client, re, *on),
            _ => self.act(client, re, op, now_us).map(Reply::Now),
        }
    }

    /// As `stop`, and its mute's connection closed, which lifts it, and
    /// the connection of each turn start still waiting for the voice
    /// shut, which drops it there: no cue and no window for a computer
    /// gone or revoked. A begun turn is left to its `turn.end` and the
    /// person, as before. Shut off the lock: a shutdown never waits.
    fn gone(&mut self, client: &str, now_us: u64) {
        self.stop(client, now_us);
        if let Some(motions) = &self.motions {
            motions.lock().expect("motions").gone(client);
        }
        let (held, turns) = {
            let mut voice = self.voice.lock().expect("voice");
            (voice.mutes.remove(client), voice.turns.remove(client))
        };
        for waiting in turns.into_iter().flatten().filter_map(|turn| turn.waiting) {
            let _ = waiting.shutdown(Shutdown::Both);
        }
        if let Some(Mute {
            held: Some(held), ..
        }) = held
        {
            let _ = held.shutdown(Shutdown::Both);
        }
    }

    fn motion_done(&self) -> bool {
        self.motions.is_some()
    }

    fn asking(&mut self, connection: Option<u64>) {
        self.asking = connection;
    }

    fn state(&self, kind: StateKind) -> Value {
        self.watch
            .as_ref()
            .and_then(|watch| watch.lock().ok().map(|watch| watch.state(kind)))
            .unwrap_or(Value::Null)
    }

    fn streaming(&self, client: &str, media: Media, turn: Option<u64>) -> bool {
        match media {
            // A camera frame always carries its stream's turn (#5881).
            Media::Camera => turn.is_some_and(|turn| {
                self.camera
                    .as_ref()
                    .is_some_and(|camera| camera.streaming(client, turn))
            }),
            Media::Microphone => self
                .microphone
                .as_ref()
                .is_some_and(|microphone| microphone.streaming(client, turn)),
        }
    }

    fn newest_turn(&self, client: &str, media: Media) -> Option<u64> {
        match media {
            Media::Camera => self.camera.as_ref().and_then(|camera| camera.turn(client)),
            Media::Microphone => self
                .microphone
                .as_ref()
                .and_then(|microphone| microphone.newest_turn(client)),
        }
    }

    fn media_end(&mut self, client: &str, media: Media) {
        match media {
            Media::Camera => {
                if let Some(camera) = &mut self.camera {
                    camera.end(client);
                }
            }
            Media::Microphone => {
                if let Some(microphone) = &mut self.microphone {
                    microphone.end(client);
                }
            }
        }
    }

    fn stop(&mut self, client: &str, _: u64) {
        if let Some(camera) = &mut self.camera {
            camera.end(client);
        }
        if let Some(microphone) = &mut self.microphone {
            microphone.end(client);
        }
        if let Some(follow) = &self.follow {
            let mut follower = follow.lock().expect("follower");
            if follower.as_deref() == Some(client) {
                *follower = None;
            }
        }
        // The light given back now: the same light, lapsing at once.
        let now_us = (self.clock)();
        if let Some((light, until_us)) = self.lights.remove(client)
            && until_us > now_us
        {
            let _ = self.ask_ring(client, light, None, now_us);
        }
        // Its own says cancelled, each on the connection that asked it, and
        // no other client's: one still connecting cancels itself when it
        // finds it is no longer kept (`ask_voice_for_say`).
        let stopped = self.says.lock().expect("says").remove(client);
        for stream in stopped.into_iter().flat_map(HashMap::into_values).flatten() {
            let _ = writeln!(&stream, "{SAY_CANCEL}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Default)]
    struct Ring(Arc<Mutex<Vec<LightRequest>>>);

    impl LightOut for Ring {
        fn publish(&mut self, request: &LightRequest) -> std::io::Result<usize> {
            self.0.lock().unwrap().push(request.clone());
            Ok(1)
        }
    }

    fn dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lamp-control-parts-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn op(frame: &str) -> Op {
        crate::protocol::parse(frame).unwrap().op
    }

    fn quiet() -> Events {
        Arc::new(|_, _| {})
    }

    #[test]
    fn a_gesture_and_a_move_go_to_the_body_at_skill_priority_as_this_part() {
        let dir = dir("body");
        let path = dir.join("mood.sock");
        let body = UnixDatagram::bind(&path).unwrap();
        let mut lamp = Lamp::new(Some(path), None, None, "boot".into(), quiet());
        lamp.act("c_1", 1, &op(r#"{"id":1,"op":"gesture","name":"nod"}"#), 0)
            .unwrap();
        let mut buffer = [0u8; 4096];
        let length = body.recv(&mut buffer).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&buffer[..length]).unwrap(),
            json!({"gesture": "nod", "replace": true, "source": "control:c_1", "priority": "skill"})
        );
        lamp.act("c_1", 2, &op(r#"{"id":2,"op":"move","step_ms":500,"frames":[[0,0,0,0,0],[20,0,30,0,0],[20,0,30,0,0],[0,0,0,0,0]]}"#), 10_000_000).unwrap();
        let length = body.recv(&mut buffer).unwrap();
        let sent: Value = serde_json::from_slice(&buffer[..length]).unwrap();
        assert_eq!(sent["move"]["frames"][1], json!([20, 0, 30, 0, 0]));
        assert_eq!(sent["priority"], "skill");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `express` is the feelings skill's own move on the mood socket and
    /// its light on the ring: glad's lift, breathing from the person's warm
    /// 3 down to 2 for 12 s; smaller below strength 0.5; with no light
    /// known, the move alone, said.
    #[test]
    fn express_is_the_feelings_skills_move_and_light() {
        let dir = dir("express");
        let path = dir.join("mood.sock");
        let body = UnixDatagram::bind(&path).unwrap();
        let ring = Ring::default();
        let watch = Arc::new(Mutex::new(crate::watch::Watch::new()));
        let mut lamp = Lamp::new(
            Some(path),
            Some(Box::new(ring.clone())),
            None,
            "boot".into(),
            quiet(),
        )
        .with_clock(booted_61s_ago)
        .watching(watch.clone());
        let glad = r#"{"id":1,"op":"express","feeling":"glad"}"#;
        // No light state yet: the move goes alone, and the answer says why.
        assert_eq!(
            lamp.act("c_1", 1, &op(glad), 0).unwrap(),
            json!({"to": "body", "move": "glad-lift", "light": "the light shown is not known"})
        );
        let mut buffer = [0u8; 16 * 1024];
        let length = body.recv(&mut buffer).unwrap();
        let sent: Value = serde_json::from_slice(&buffer[..length]).unwrap();
        assert_eq!(
            sent["move"],
            json!(crate::feelings::named("glad").unwrap().written())
        );
        assert_eq!(
            (&sent["source"], &sent["priority"]),
            (&json!("control:c_1"), &json!("skill"))
        );
        assert!(ring.0.lock().unwrap().is_empty());
        watch.lock().unwrap().take(
            "light/state",
            &json!({"on": true, "colour": "warm", "level": 3}),
            0,
        );
        assert_eq!(
            lamp.act(
                "c_1",
                2,
                &op(r#"{"id":2,"op":"express","feeling":"glad","strength":0.25}"#),
                60_000_000
            )
            .unwrap(),
            json!({"to": "body", "move": "glad-lift-50", "light": true})
        );
        let length = body.recv(&mut buffer).unwrap();
        let sent: Value = serde_json::from_slice(&buffer[..length]).unwrap();
        let written = crate::feelings::named("glad").unwrap().written();
        let peak = |frames: &Value| {
            frames
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|frame| {
                    frame
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|count| count.as_i64().unwrap().abs())
                })
                .max()
                .unwrap()
        };
        assert!(peak(&sent["move"]["frames"]) < peak(&json!(written.frames)));
        let asked = ring.0.lock().unwrap();
        assert_eq!(asked[0].claim.until_us, 61_000_000 + 12_000_000);
        assert_eq!(
            asked[0].light,
            Light {
                on: true,
                colour: "warm".into(),
                level: 3
            }
        );
        assert_eq!(
            asked[0].pattern,
            Some(LightPattern {
                kind: PatternKind::Breathe,
                period_ms: 6_000,
                low: 2
            })
        );
        drop(asked);
        // A feeling the skill doesn't have.
        assert_eq!(
            lamp.act(
                "c_1",
                3,
                &op(r#"{"id":3,"op":"express","feeling":"cross"}"#),
                0
            )
            .unwrap_err()
            .code,
            ErrorCode::BadRequest
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A lamp with a mood socket the test reads, and the socket.
    fn with_body(name: &str) -> (Lamp, UnixDatagram, PathBuf) {
        let dir = dir(name);
        let path = dir.join("mood.sock");
        let body = UnixDatagram::bind(&path).unwrap();
        body.set_nonblocking(true).unwrap();
        (
            Lamp::new(Some(path), None, None, "boot".into(), quiet()),
            body,
            dir,
        )
    }

    const NOD: &str = r#"{"id":1,"op":"gesture","name":"nod"}"#;

    fn asks(body: &UnixDatagram) -> Vec<Value> {
        let mut buffer = [0u8; 16 * 1024];
        std::iter::from_fn(|| {
            let length = body.recv(&mut buffer).ok()?;
            Some(serde_json::from_slice(&buffer[..length]).unwrap())
        })
        .collect()
    }

    /// A computer's new motion interrupts the one playing: two gestures
    /// 100 ms apart, from one computer or two, are both sent, each marked
    /// to replace; so are a move and an express move asked over them. The
    /// body decides (it refuses while the person is using the lamp).
    #[test]
    fn a_new_motion_is_sent_marked_to_replace_the_one_playing() {
        let (mut lamp, body, dir) = with_body("replace");
        lamp.act("c_1", 1, &op(NOD), 1_000_000).unwrap();
        lamp.act("c_2", 2, &op(NOD), 1_100_000).unwrap();
        let walk = op(
            r#"{"id":3,"op":"move","step_ms":500,"frames":[[0,0,0,0,0],[20,0,30,0,0],[20,0,30,0,0],[0,0,0,0,0]]}"#,
        );
        lamp.act("c_1", 3, &walk, 1_200_000).unwrap();
        lamp.act(
            "c_1",
            4,
            &op(r#"{"id":4,"op":"express","feeling":"glad"}"#),
            1_300_000,
        )
        .unwrap();
        let sent = asks(&body);
        assert_eq!(sent.len(), 4, "{sent:?}");
        assert_eq!(
            sent[1],
            json!({"gesture": "nod", "replace": true, "source": "control:c_2", "priority": "skill"})
        );
        assert!(sent.iter().all(|ask| ask["replace"] == true), "{sent:?}");
        assert!(sent[2]["move"].is_object() && sent[3]["move"].is_object());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 61 s since boot.
    fn booted_61s_ago() -> u64 {
        61_000_000
    }

    /// The Reviewer-lead's P2 on #3805: the door started a minute after
    /// boot, so its own clock reads 1 s; the claim is on the boot's clock,
    /// the one the ring reads.
    #[test]
    fn a_light_is_a_request_that_lapses_and_stop_gives_it_back() {
        let ring = Ring::default();
        let mut lamp = Lamp::new(
            None,
            Some(Box::new(ring.clone())),
            None,
            "boot-1".into(),
            quiet(),
        )
        .with_clock(booted_61s_ago);
        lamp.act("c_1", 1, &op(r#"{"id":1,"op":"light","colour":"warm","level":3,"pattern":"breathe","for_ms":10000}"#), 1_000_000).unwrap();
        {
            let asked = ring.0.lock().unwrap();
            assert_eq!(
                asked[0].claim,
                Claim {
                    source: "control:c_1".into(),
                    priority: Priority::Skill,
                    clock_id: "boot-1".into(),
                    until_us: 71_000_000
                }
            );
            assert_eq!(
                asked[0].light,
                Light {
                    on: true,
                    colour: "warm".into(),
                    level: 3
                }
            );
            assert_eq!(
                asked[0].pattern.map(|p| (p.kind, p.period_ms)),
                Some((PatternKind::Breathe, 6_000))
            );
        }
        // A candle at level 2 is the ring's own refusal, said.
        let error = lamp.act("c_1", 2, &op(r#"{"id":2,"op":"light","colour":"warm","level":2,"pattern":"candle","for_ms":1000}"#), 0).unwrap_err();
        assert_eq!(error.code, ErrorCode::Refused);
        assert!(error.why.contains("level 3"), "{}", error.why);
        lamp.stop("c_1", 2_000_000);
        let asked = ring.0.lock().unwrap();
        assert_eq!(asked.len(), 2);
        assert_eq!(asked[1].claim.until_us, 61_000_000);
    }

    /// "The ring, pixel by pixel": a computer's own colour or pixels go to
    /// the ring as given, at their brightness, under the same claim as a
    /// named light, with the nearest named light beside them for a ring
    /// that draws none.
    #[test]
    fn a_computers_own_pixels_go_to_the_ring_at_their_brightness() {
        let ring = Ring::default();
        let mut lamp = Lamp::new(
            None,
            Some(Box::new(ring.clone())),
            None,
            "boot".into(),
            quiet(),
        )
        .with_clock(booted_61s_ago);
        let mut pixels = vec!["#000000"; 32];
        pixels[0] = "#ff0000";
        pixels[1] = "#00ff80";
        let frame =
            json!({"id": 1, "op": "light", "pixels": pixels, "brightness": 50, "for_ms": 3000});
        assert_eq!(
            lamp.act("c_1", 1, &op(&frame.to_string()), 0).unwrap(),
            json!({"to": "ring", "pixels": 32})
        );
        let rgb = r##"{"id":2,"op":"light","rgb":"#2040ff","fade_ms":2000,"for_ms":1000}"##;
        lamp.act("c_1", 2, &op(rgb), 0).unwrap();
        let asked = ring.0.lock().unwrap();
        let first = &asked[0];
        let drawn = first.pixels.as_ref().unwrap();
        assert_eq!(
            (drawn.len(), drawn[0], drawn[1], drawn[2]),
            (32, [128, 0, 0], [0, 128, 64], [0, 0, 0])
        );
        assert_eq!(first.claim.priority, Priority::Skill);
        assert_eq!(first.claim.until_us, 64_000_000);
        assert!(first.pattern.is_none() && first.check().is_ok());
        // Red and teal, half bright: their mean is near white, at level 3,
        // on a ring without pixels.
        assert_eq!(
            (
                first.light.on,
                first.light.colour.as_str(),
                first.light.level
            ),
            (true, "white", 3)
        );
        assert_eq!(nearest_light(&[[200, 10, 0]; 32]).colour, "red");
        let second = &asked[1];
        assert_eq!(
            second.pixels.as_ref().unwrap(),
            &vec![[0x20, 0x40, 0xff]; 32]
        );
        assert_eq!(
            (second.fade_ms, second.light.colour.as_str()),
            (Some(2000), "blue")
        );
    }

    /// lamp-actions' call socket answering `calls` the way it does: each
    /// asked line is kept, and the level moves as the voice's own steps do.
    fn actions(name: &str, calls: usize) -> (PathBuf, std::thread::JoinHandle<Vec<Value>>) {
        let dir = dir(name);
        let path = dir.join("call.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let serving = std::thread::spawn(move || {
            let (mut level, mut before) = (50u64, 50u64);
            let mut asked = Vec::new();
            for _ in 0..calls {
                let (stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                let call: Value = serde_json::from_str(&line).unwrap();
                match (call["name"].as_str().unwrap(), &call["args"]) {
                    ("volume_to", args) => level = args["volume"].as_u64().unwrap(),
                    ("volume_step", args) if args["step"] == "up" => level = (level + 20).min(100),
                    ("volume_step", _) => level = level.saturating_sub(20).max(10),
                    ("volume_muted", args) if args["muted"] == true => (before, level) = (level, 0),
                    ("volume_muted", _) => level = before,
                    _ => {}
                }
                let mut out = &stream;
                writeln!(
                    out,
                    "{}",
                    json!({"ok": true, "level": {"volume": level, "raw": level, "max_volume": 100}})
                )
                .unwrap();
                asked.push(call);
            }
            asked
        });
        (path, serving)
    }

    #[test]
    fn the_volume_op_sets_steps_and_mutes_through_lamp_actions_app_calls() {
        let (path, serving) = actions("volume", 4);
        let mut lamp = Lamp::new(None, None, None, "boot".into(), quiet()).volume_through(path);
        let mut ask = |frame: &str| lamp.act("c_1", 1, &op(frame), 0).unwrap();
        assert_eq!(
            ask(r#"{"id":1,"op":"volume","to":35}"#),
            json!({"level": 35, "muted": false})
        );
        assert_eq!(
            ask(r#"{"id":2,"op":"volume","step":"up"}"#),
            json!({"level": 55, "muted": false})
        );
        assert_eq!(
            ask(r#"{"id":3,"op":"volume","mute":true}"#),
            json!({"level": 0, "muted": true})
        );
        assert_eq!(
            ask(r#"{"id":4,"op":"volume","mute":false}"#),
            json!({"level": 55, "muted": false})
        );
        let asked = serving.join().unwrap();
        assert_eq!(
            asked
                .iter()
                .map(|call| (call["name"].clone(), call["args"].clone()))
                .collect::<Vec<_>>(),
            [
                (json!("volume_to"), json!({"volume": 35})),
                (json!("volume_step"), json!({"step": "up"})),
                (json!("volume_muted"), json!({"muted": true})),
                (json!("volume_muted"), json!({"muted": false})),
            ]
        );
    }

    #[test]
    fn every_pixel_black_is_the_light_off_and_a_named_light_is_as_before() {
        let ring = Ring::default();
        let mut lamp = Lamp::new(
            None,
            Some(Box::new(ring.clone())),
            None,
            "boot".into(),
            quiet(),
        )
        .with_clock(booted_61s_ago);
        let off = json!({"id": 1, "op": "light", "pixels": vec!["#000000"; 32], "for_ms": 1000});
        lamp.act("c_1", 1, &op(&off.to_string()), 0).unwrap();
        let named = r#"{"id":2,"op":"light","colour":"warm","level":2,"for_ms":1000}"#;
        lamp.act("c_1", 2, &op(named), 0).unwrap();
        let asked = ring.0.lock().unwrap();
        assert!(!asked[0].light.on && asked[0].check().is_ok());
        assert_eq!(asked[1].pixels, None);
        assert_eq!(
            (asked[1].light.colour.as_str(), asked[1].light.level),
            ("warm", 2)
        );
    }

    #[test]
    fn a_volume_without_lamp_actions_or_refused_by_it_says_why() {
        let mut lamp = Lamp::new(None, None, None, "boot".into(), quiet());
        let error = lamp
            .act("c_1", 1, &op(r#"{"id":1,"op":"volume","to":10}"#), 0)
            .unwrap_err();
        assert!(error.why.contains("not reachable"), "{}", error.why);
        let dir = dir("volume-refused");
        let path = dir.join("call.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let actions = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            let mut out = &stream;
            writeln!(
                out,
                r#"{{"ok":false,"error":"this lamp has no volume control"}}"#
            )
            .unwrap();
        });
        let mut lamp = Lamp::new(None, None, None, "boot".into(), quiet()).volume_through(path);
        let error = lamp
            .act("c_1", 1, &op(r#"{"id":1,"op":"volume","step":"down"}"#), 0)
            .unwrap_err();
        actions.join().unwrap();
        assert_eq!(error.code, ErrorCode::Refused);
        assert!(error.why.contains("no volume control"), "{}", error.why);
    }

    #[test]
    fn a_say_is_written_words_and_its_outcome_comes_as_done() {
        let dir = dir("say");
        let path = dir.join("say.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let told: Arc<Mutex<Vec<(String, Value)>>> = Arc::default();
        let sink = told.clone();
        let events: Events =
            Arc::new(move |client, event| sink.lock().unwrap().push((client.to_owned(), event)));
        let voice = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            let mut out = &stream;
            writeln!(out, r#"{{"event":"begun","at_us":5}}"#).unwrap();
            writeln!(out, r#"{{"outcome":"spoken"}}"#).unwrap();
            serde_json::from_str::<Value>(&line).unwrap()
        });
        let mut lamp = Lamp::new(None, None, Some(path), "boot".into(), events);
        assert_eq!(
            lamp.act(
                "c_1",
                14,
                &op(r#"{"id":14,"op":"say","text":"Build passed."}"#),
                0
            )
            .unwrap(),
            json!({"to": "voice"})
        );
        let asked = voice.join().unwrap();
        assert_eq!(
            asked,
            json!({"skill": "control", "text": "Build passed.", "expires_in_ms": 30000, "exact": true})
        );
        for _ in 0..200 {
            if !told.lock().unwrap().is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            told.lock().unwrap()[0],
            (
                "c_1".to_owned(),
                json!({"ev": "done", "re": 14, "how": "finished"})
            )
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A client's `stop` cancels its own say on the connection that asked
    /// it, and its `done` is dropped with the voice's why; another client's
    /// say on the same voice is untouched and still answered.
    #[test]
    fn a_stop_cancels_the_clients_own_say_and_no_other() {
        let dir = dir("say-stop");
        let path = dir.join("say.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let told: Arc<Mutex<Vec<(String, Value)>>> = Arc::default();
        let sink = told.clone();
        let events: Events =
            Arc::new(move |client, event| sink.lock().unwrap().push((client.to_owned(), event)));
        let mut lamp = Lamp::new(None, None, Some(path), "boot".into(), events);
        lamp.act("c_1", 1, &op(r#"{"id":1,"op":"say","text":"Mine."}"#), 0)
            .unwrap();
        lamp.act("c_2", 2, &op(r#"{"id":2,"op":"say","text":"Theirs."}"#), 0)
            .unwrap();
        // The voice's side: each connection by its words.
        let mut by_text = HashMap::new();
        for _ in 0..2 {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let asked: Value = serde_json::from_str(&line).unwrap();
            by_text.insert(asked["text"].as_str().unwrap().to_owned(), (stream, reader));
        }
        // Both kept with their connections before the stop.
        for _ in 0..200 {
            let kept = lamp
                .says
                .lock()
                .unwrap()
                .values()
                .flat_map(HashMap::values)
                .flatten()
                .count();
            if kept == 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        lamp.stop("c_1", 0);
        let (mine, mine_reader) = by_text.get_mut("Mine.").unwrap();
        let mut line = String::new();
        mine_reader.read_line(&mut line).unwrap();
        assert_eq!(line.trim_end(), SAY_CANCEL);
        writeln!(
            mine,
            r#"{{"outcome":"dropped","why":"{STOPPED_BY_ASKER}"}}"#
        )
        .unwrap();
        // Theirs hears nothing from c_1's stop.
        let (theirs, theirs_reader) = by_text.get_mut("Theirs.").unwrap();
        theirs
            .set_read_timeout(Some(Duration::from_millis(300)))
            .unwrap();
        let mut line = String::new();
        assert!(theirs_reader.read_line(&mut line).is_err(), "{line}");
        writeln!(theirs, r#"{{"outcome":"spoken"}}"#).unwrap();
        for _ in 0..200 {
            if told.lock().unwrap().len() == 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut told = told.lock().unwrap().clone();
        told.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            told,
            vec![
                (
                    "c_1".to_owned(),
                    json!({"ev": "done", "re": 1, "how": "dropped",
                        "why": format!("dropped: {STOPPED_BY_ASKER}")})
                ),
                (
                    "c_2".to_owned(),
                    json!({"ev": "done", "re": 2, "how": "finished"})
                ),
            ]
        );
        // Answered, neither is kept for a later stop.
        assert!(lamp.says.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `look` at `hand` follows it through the lease socket's hold; any
    /// other look taken, or its own client's `stop`, ends that; a refused
    /// look doesn't (Nine's P3 on #4375).
    #[test]
    fn a_look_at_the_hand_follows_it_until_another_look_or_a_stop() {
        let at_hand = op(r#"{"id":1,"op":"look","at":"hand"}"#);
        let mut lamp = Lamp::new(None, None, None, "boot".into(), quiet());
        let error = lamp.act("c_1", 1, &at_hand, 0).unwrap_err();
        assert!(error.why.contains("lease socket"), "{}", error.why);
        let follow = crate::body_lease::Follower::default();
        let following = || follow.lock().unwrap().clone();
        let mut lamp =
            Lamp::new(None, None, None, "boot".into(), quiet()).following(follow.clone());
        assert_eq!(
            lamp.act("c_1", 1, &at_hand, 0).unwrap(),
            json!({"to": "body", "follow": "hand"})
        );
        assert_eq!(following().as_deref(), Some("c_1"));
        // Refused looks leave it: one out of this release, and one with no
        // lease socket to take it.
        let face = op(r#"{"id":2,"op":"look","at":"face"}"#);
        assert!(lamp.act("c_1", 2, &face, 0).is_err());
        let ahead = op(r#"{"id":2,"op":"look","at":"ahead"}"#);
        assert!(lamp.act("c_1", 2, &ahead, 0).is_err());
        assert_eq!(following().as_deref(), Some("c_1"), "a refused look");
        let (looks, _looked) = tokio::sync::mpsc::unbounded_channel();
        let mut lamp = Lamp::new(None, None, None, "boot".into(), quiet())
            .following(follow.clone())
            .looking(looks);
        lamp.act("c_1", 2, &at_hand, 0).unwrap();
        assert_eq!(
            lamp.act("c_1", 2, &ahead, 0).unwrap(),
            json!({"to": "body"})
        );
        assert_eq!(following(), None, "another look taken ends it");
        lamp.act("c_1", 3, &at_hand, 0).unwrap();
        lamp.stop("c_2", 0);
        assert_eq!(following().as_deref(), Some("c_1"), "another client's stop");
        lamp.stop("c_1", 0);
        assert_eq!(following(), None);
    }

    /// `look` goes to the door's hold of the body's lease, as a line the
    /// body reads, with who asked and the request it answers.
    #[test]
    fn a_look_goes_on_the_bodys_lease_as_its_line() {
        let (looks, mut looked) = tokio::sync::mpsc::unbounded_channel();
        let mut lamp = Lamp::new(None, None, None, "boot".into(), quiet()).looking(looks);
        assert_eq!(
            lamp.act(
                "c_1",
                10,
                &op(r#"{"id":10,"op":"look","at":{"yaw":-10,"pitch":5}}"#),
                0
            )
            .unwrap(),
            json!({"to": "body"})
        );
        let look = looked.try_recv().unwrap();
        assert_eq!((look.client.as_str(), look.re), ("c_1", 10));
        assert_eq!(
            serde_json::from_str::<Value>(&look.line).unwrap(),
            json!({"look": {"yaw": -10.0, "pitch": 5.0}})
        );
        lamp.act("c_1", 11, &op(r#"{"id":11,"op":"look","at":"ahead"}"#), 0)
            .unwrap();
        assert_eq!(looked.try_recv().unwrap().line, r#"{"look":"ahead"}"#);
        // A face: the body's own line, for it to turn once toward the face
        // the eyes last showed (row 34); a name is out of this release.
        lamp.act("c_1", 12, &op(r#"{"id":12,"op":"look","at":"face"}"#), 0)
            .unwrap();
        assert_eq!(looked.try_recv().unwrap().line, r#"{"look":"face"}"#);
        let error = lamp
            .act("c_1", 13, &op(r#"{"id":13,"op":"look","at":"name"}"#), 0)
            .unwrap_err();
        assert_eq!(error.why, NOT_IN_THIS_RELEASE);
        assert!(looked.try_recv().is_err(), "nothing sent for a name");
    }

    /// `sound` is the lamp's own short hum: the voice's built-in cue on the
    /// say socket, with say's gates and outcome (in this release, #3504).
    #[test]
    fn a_sound_is_the_voices_own_hum_and_its_outcome_comes_as_done() {
        let dir = dir("sound");
        let path = dir.join("say.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let told: Arc<Mutex<Vec<(String, Value)>>> = Arc::default();
        let sink = told.clone();
        let events: Events =
            Arc::new(move |client, event| sink.lock().unwrap().push((client.to_owned(), event)));
        let voice = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            let mut out = &stream;
            writeln!(out, r#"{{"event":"begun","at_us":5}}"#).unwrap();
            writeln!(out, r#"{{"outcome":"spoken"}}"#).unwrap();
            serde_json::from_str::<Value>(&line).unwrap()
        });
        let mut lamp = Lamp::new(None, None, Some(path), "boot".into(), events);
        assert_eq!(
            lamp.act("c_1", 14, &op(r#"{"id":14,"op":"sound","name":"hum"}"#), 0)
                .unwrap(),
            json!({"to": "voice"})
        );
        let asked = voice.join().unwrap();
        assert_eq!(
            asked,
            json!({"skill": "control", "cue": "hum", "expires_in_ms": 30000})
        );
        for _ in 0..200 {
            if !told.lock().unwrap().is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            told.lock().unwrap()[0],
            (
                "c_1".to_owned(),
                json!({"ev": "done", "re": 14, "how": "finished"})
            )
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A computer's own clip, by name: its path in the clips folder sent to
    /// the voice as `clip`, never one of another computer's; one not kept,
    /// or on a lamp keeping none, is refused with why and nothing is sent.
    #[test]
    fn a_clip_is_sent_to_the_voice_by_its_path_in_the_computers_folder() {
        let dir = dir("clip");
        let clips = dir.join("sounds");
        std::fs::create_dir_all(clips.join("c_1")).unwrap();
        std::fs::write(clips.join("c_1").join("chime.wav"), b"RIFF").unwrap();
        let path = dir.join("say.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let events: Events = Arc::new(|_, _| {});
        let voice = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            writeln!(&stream, r#"{{"outcome":"spoken"}}"#).unwrap();
            serde_json::from_str::<Value>(&line).unwrap()
        });
        let mut none = Lamp::new(
            None,
            None,
            Some(path.clone()),
            "boot".into(),
            events.clone(),
        );
        assert_eq!(
            none.act("c_1", 1, &op(r#"{"id":1,"op":"sound","name":"chime"}"#), 0)
                .unwrap_err()
                .why,
            crate::session::SOUNDS_OFF
        );
        let mut lamp =
            Lamp::new(None, None, Some(path), "boot".into(), events).with_clips(clips.clone());
        assert!(
            lamp.act("c_2", 1, &op(r#"{"id":1,"op":"sound","name":"chime"}"#), 0)
                .unwrap_err()
                .why
                .starts_with("no clip named chime")
        );
        assert_eq!(
            lamp.act("c_1", 2, &op(r#"{"id":2,"op":"sound","name":"chime"}"#), 0)
                .unwrap(),
            json!({"to": "voice"})
        );
        assert_eq!(
            voice.join().unwrap(),
            json!({
                "skill": "control",
                "clip": clips.join("c_1").join("chime.wav"),
                "expires_in_ms": 30000,
            })
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `joints`' yaw and pitch go as the holder's look (the same degrees
    /// from home); a joint not named stays where `get` `body` says it is;
    /// roll and the arm go as the holder's pose, and one naming both sends
    /// both lines under one join.
    #[test]
    fn joints_yaw_and_pitch_go_as_the_holders_look() {
        let (looks, mut looked) = tokio::sync::mpsc::unbounded_channel();
        let watch = Arc::new(std::sync::Mutex::new(crate::watch::Watch::new()));
        let mut lamp = Lamp::new(None, None, None, "boot".into(), quiet())
            .watching(watch.clone())
            .looking(looks);
        let joints = |frame: &str| op(frame);
        lamp.act(
            "c_1",
            12,
            &joints(r#"{"id":12,"op":"joints","lease":"l_1","targets":{"yaw":-10,"pitch":5},"within_ms":300}"#),
            0,
        )
        .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&looked.try_recv().unwrap().line).unwrap(),
            json!({"look": {"yaw": -10.0, "pitch": 5.0, "from": "home"}})
        );
        // Only yaw, before the body has said where it is: refused.
        let error = lamp
            .act(
                "c_1",
                13,
                &joints(
                    r#"{"id":13,"op":"joints","lease":"l_1","targets":{"yaw":4},"within_ms":300}"#,
                ),
                0,
            )
            .unwrap_err();
        assert!(error.why.contains("name both"), "{}", error.why);
        // Once it has: the pitch stays where it is.
        {
            let mut watch = watch.lock().unwrap();
            let limits = json!({"joints": [{"name": "head_pitch", "min": 1955, "max": 2015, "home": 2000}, {"name": "base_yaw", "min": 1975, "max": 2055, "home": 2015}], "speed": 50.0, "acceleration": 400.0, "heat_stop": 62, "stands": false});
            watch.take("body/limits", &limits, 0);
            let tau = std::f64::consts::TAU;
            let radians = |counts: f64| (counts - 2_048.0) * tau / 4_096.0;
            let joints = json!({"name": ["base_yaw", "head_pitch"], "position": [radians(2_015.0), radians(1_966.0)]});
            watch.take("body/joint_states", &joints, 0);
        }
        lamp.act(
            "c_1",
            14,
            &joints(r#"{"id":14,"op":"joints","lease":"l_1","targets":{"yaw":4},"within_ms":300}"#),
            0,
        )
        .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&looked.try_recv().unwrap().line).unwrap(),
            json!({"look": {"yaw": 4.0, "pitch": 3.0, "from": "home"}})
        );
        // Only the arm: one pose line, no look, no join.
        lamp.act(
            "c_1",
            15,
            &joints(r#"{"id":15,"op":"joints","lease":"l_1","targets":{"roll":3,"shoulder":10},"within_ms":300}"#),
            0,
        )
        .unwrap();
        let pose = looked.try_recv().unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&pose.line).unwrap(),
            json!({"pose": {"roll": 3.0, "shoulder": 10.0}})
        );
        assert!(pose.join.is_none() && looked.try_recv().is_err());
        // The head and the elbow: a look and a pose, sharing one join.
        lamp.act(
            "c_1",
            17,
            &joints(r#"{"id":17,"op":"joints","lease":"l_1","targets":{"yaw":2,"pitch":1,"elbow":-5},"within_ms":300}"#),
            0,
        )
        .unwrap();
        let (look, pose) = (looked.try_recv().unwrap(), looked.try_recv().unwrap());
        assert_eq!(
            serde_json::from_str::<Value>(&look.line).unwrap(),
            json!({"look": {"yaw": 2.0, "pitch": 1.0, "from": "home"}})
        );
        assert_eq!(
            serde_json::from_str::<Value>(&pose.line).unwrap(),
            json!({"pose": {"elbow": -5.0}})
        );
        assert!(
            look.join
                .is_some_and(|join| pose.join.is_some_and(|other| Arc::ptr_eq(&join, &other)))
        );
        // The body not heard for 15 s (stopped or restarted): the unnamed
        // pitch is refused, never sent from where the head was before.
        watch.lock().unwrap().lapse(crate::watch::READING_STALE_US);
        let error = lamp
            .act(
                "c_1",
                16,
                &joints(
                    r#"{"id":16,"op":"joints","lease":"l_1","targets":{"yaw":4},"within_ms":300}"#,
                ),
                0,
            )
            .unwrap_err();
        assert!(error.why.contains("name both"), "{}", error.why);
        assert!(looked.try_recv().is_err(), "nothing sent");
        // Both named still go.
        lamp.act(
            "c_1",
            17,
            &joints(r#"{"id":17,"op":"joints","lease":"l_1","targets":{"yaw":4,"pitch":2},"within_ms":300}"#),
            0,
        )
        .unwrap();
        assert!(looked.try_recv().is_ok());
    }

    #[test]
    fn what_is_not_routed_yet_says_what_it_waits_for() {
        let mut lamp = Lamp::new(None, None, None, "boot".into(), quiet());
        for (frame, waits) in [
            (
                r#"{"id":1,"op":"look","at":"ahead"}"#,
                "lease socket is not on",
            ),
            (r#"{"id":1,"op":"look","at":"name"}"#, NOT_IN_THIS_RELEASE),
            (r#"{"id":1,"op":"play","name":"wave"}"#, "packages"),
        ] {
            let error = lamp.act("c_1", 1, &op(frame), 0).unwrap_err();
            assert!(error.why.contains(waits), "{frame}: {}", error.why);
        }
        let error = lamp
            .act("c_1", 1, &op(r#"{"id":1,"op":"gesture","name":"nod"}"#), 0)
            .unwrap_err();
        assert!(error.why.contains("not reachable"), "{}", error.why);
        assert_eq!(
            outcome(BufReader::new(
                &b"{\"event\":\"begun\"}\n{\"outcome\":\"dropped\",\"why\":\"someone spoke\"}\n"[..]
            )),
            ("dropped", Some("dropped: someone spoke".to_owned()))
        );
        // The voice's words in the protocol's (the P2s on #3805).
        for (line, how, why) in [
            (r#"{"outcome":"spoken"}"#, "finished", None),
            (r#"{"outcome":"expired"}"#, "dropped", Some("expired")),
            (
                r#"{"outcome":"refused","why":"private"}"#,
                "dropped",
                Some("refused: private"),
            ),
            ("", "dropped", Some("the voice gave no outcome")),
            // A computer's turn ended by the person beginning to talk with
            // the lamp in its window: cut short, the person wins.
            (
                r#"{"outcome":"dropped","why":"the person is talking"}"#,
                "cut_short",
                Some("the person is talking"),
            ),
            // Its answer cut by its own computer's turn.end.
            (
                r#"{"outcome":"dropped","why":"ended by this computer"}"#,
                "cut_short",
                Some("ended by this computer"),
            ),
        ] {
            assert_eq!(
                outcome(BufReader::new(line.as_bytes())),
                (how, why.map(str::to_owned)),
                "{line}"
            );
        }
    }

    /// A voice that can't be reached is a `done` dropped, off the door's
    /// lock: the reply comes at once.
    #[test]
    fn a_say_the_voice_cannot_take_is_done_dropped() {
        let dir = dir("say-gone");
        let told: Arc<Mutex<Vec<(String, Value)>>> = Arc::default();
        let sink = told.clone();
        let events: Events =
            Arc::new(move |client, event| sink.lock().unwrap().push((client.to_owned(), event)));
        let mut lamp = Lamp::new(
            None,
            None,
            Some(dir.join("none.sock")),
            "boot".into(),
            events,
        );
        let said = lamp.act("c_1", 3, &op(r#"{"id":3,"op":"say","text":"Hi."}"#), 0);
        assert_eq!(said.unwrap(), json!({"to": "voice"}));
        for _ in 0..200 {
            if !told.lock().unwrap().is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let (client, done) = told.lock().unwrap()[0].clone();
        assert_eq!(client, "c_1");
        assert_eq!(done["how"], "dropped");
        assert!(
            done["why"].as_str().unwrap().contains("did not take"),
            "{done}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Events kept as the door would send them, by client.
    type Told = Arc<Mutex<Vec<(String, Value)>>>;

    fn kept() -> (Events, Told) {
        let told: Told = Arc::default();
        let sink = told.clone();
        let events: Events =
            Arc::new(move |client, event| sink.lock().unwrap().push((client.to_owned(), event)));
        (events, told)
    }

    fn until_told(told: &Told, count: usize) {
        for _ in 0..300 {
            if told.lock().unwrap().len() >= count {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("told {:?}", told.lock().unwrap());
    }

    /// A connection to the fake voice and its one line read.
    fn accepted(listener: &std::os::unix::net::UnixListener) -> (UnixStream, Value) {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line).unwrap();
        (stream, serde_json::from_str(&line).unwrap())
    }

    /// Nobody connected to the fake voice.
    fn untouched(listener: &std::os::unix::net::UnixListener) {
        listener.set_nonblocking(true).unwrap();
        assert_eq!(
            listener.accept().map(|_| ()).unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        listener.set_nonblocking(false).unwrap();
    }

    fn say(stream: &UnixStream, line: &str) {
        let mut out = stream;
        writeln!(out, "{line}").unwrap();
    }

    fn now(reply: Result<Reply, Error>) -> Value {
        match reply.unwrap() {
            Reply::Now(with) => with,
            Reply::Later(_) => panic!("a reply now was wanted"),
        }
    }

    /// The part's wait, on a thread as the door runs it.
    fn later(reply: Result<Reply, Error>) -> std::thread::JoinHandle<Result<Value, Error>> {
        match reply.unwrap() {
            Reply::Later(run) => std::thread::spawn(run),
            Reply::Now(with) => panic!("a reply later was wanted, not {with}"),
        }
    }

    /// docs/control.md, "Turns and listening": a turn goes to the voice as
    /// its `turn` line, prompt as `text`; its id from `begun` is kept for its
    /// computer alone, so `turn.end` names it and another computer's end is
    /// refused without asking the voice; the outcome is the start's `done`.
    #[test]
    fn a_turn_is_asked_of_the_voice_and_only_its_computer_ends_it() {
        let dir = dir("turn");
        let path = dir.join("say.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let (events, told) = kept();
        let mut lamp = Lamp::new(None, None, Some(path), "boot".into(), events);
        let end = op(r#"{"id":4,"op":"turn.end"}"#);
        // No turn of its own: refused, the voice not asked.
        let refused = lamp.reply("c_1", 4, &end, 0).unwrap_err();
        assert_eq!(
            (refused.code, refused.why.as_str()),
            (ErrorCode::Busy, NOT_THIS_TURN)
        );
        let start = op(r#"{"id":3,"op":"turn.start","prompt":"Shall we go over today's list?"}"#);
        assert_eq!(
            lamp.act("c_1", 3, &start, 0).unwrap(),
            json!({"to": "voice"})
        );
        let (turn, line) = accepted(&listener);
        assert_eq!(
            line,
            json!({"skill": "control", "turn": "start", "text": "Shall we go over today's list?", "expires_in_ms": 30000})
        );
        say(&turn, r#"{"event":"begun","at_us":5,"id":7}"#);
        for _ in 0..300 {
            if begun_id(&lamp, "c_1") == Some(7) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        // Another computer's end: refused, the voice not asked.
        let other = lamp.reply("c_2", 9, &end, 0).unwrap_err();
        assert_eq!(
            (other.code, other.why.as_str()),
            (ErrorCode::Busy, NOT_THIS_TURN)
        );
        untouched(&listener);
        let ending = later(lamp.reply("c_1", 4, &end, 0));
        let (asked, line) = accepted(&listener);
        assert_eq!(line, json!({"skill": "control", "turn": "end", "id": 7}));
        say(&asked, r#"{"outcome":"ended"}"#);
        assert_eq!(
            ending.join().unwrap().unwrap(),
            json!({"turn": "ended", "started": 3})
        );
        say(
            &turn,
            r#"{"outcome":"dropped","why":"ended by the computer"}"#,
        );
        until_told(&told, 1);
        assert_eq!(
            told.lock().unwrap()[0],
            (
                "c_1".to_owned(),
                json!({"ev": "done", "re": 3, "how": "dropped", "why": "dropped: ended by the computer"})
            )
        );
        // Ended by its end, the turn is no longer its to end.
        let after = lamp.reply("c_1", 5, &end, 0).unwrap_err();
        assert_eq!(after.why, NOT_THIS_TURN);
        // Without a prompt, no text; spoken is finished.
        lamp.act("c_1", 6, &op(r#"{"id":6,"op":"turn.start"}"#), 0)
            .unwrap();
        let (turn, line) = accepted(&listener);
        assert_eq!(
            line,
            json!({"skill": "control", "turn": "start", "expires_in_ms": 30000})
        );
        say(&turn, r#"{"event":"begun","at_us":5,"id":8}"#);
        say(&turn, r#"{"outcome":"spoken"}"#);
        until_told(&told, 2);
        assert_eq!(
            told.lock().unwrap()[1].1,
            json!({"ev": "done", "re": 6, "how": "finished"})
        );
        // A voice from before turns refuses the field: said so.
        lamp.act("c_1", 7, &op(r#"{"id":7,"op":"turn.start"}"#), 0)
            .unwrap();
        let (turn, _) = accepted(&listener);
        say(&turn, r#"{"outcome":"refused","why":"not a say request"}"#);
        until_told(&told, 3);
        assert_eq!(
            told.lock().unwrap()[2].1,
            json!({"ev": "done", "re": 7, "how": "dropped", "why": format!("refused: {NO_TURNS}")})
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn begun_id(lamp: &Lamp, client: &str) -> Option<u64> {
        lamp.voice
            .lock()
            .unwrap()
            .begun
            .get(client)
            .map(|begun| begun.id)
    }

    fn until_begun(lamp: &Lamp, client: &str, id: u64) {
        for _ in 0..300 {
            if begun_id(lamp, client) == Some(id) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("{client}'s turn {id} never begun");
    }

    /// lamp-75c9, 3 Nov 19:27: a computer's next turn.start, sent while its
    /// earlier turn's conversation went on, names that turn as `after`; the
    /// voice refusing it kept no id, so the computer's turn.end was refused
    /// "not this computer's turn" with its conversation still going on. A
    /// start refused before its `begun` leaves the earlier id, which its
    /// outcome never clears; a start begun replaces it.
    #[test]
    fn a_later_start_names_the_earlier_turn_and_only_its_begun_replaces_it() {
        let dir = dir("turn-after");
        let path = dir.join("say.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let (events, told) = kept();
        let mut lamp = Lamp::new(None, None, Some(path), "boot".into(), events);
        let start = |re: u64| op(&format!(r#"{{"id":{re},"op":"turn.start"}}"#));
        // The first turn: no earlier one to name.
        lamp.act("c_1", 3, &start(3), 0).unwrap();
        let (first, line) = accepted(&listener);
        assert_eq!(
            line,
            json!({"skill": "control", "turn": "start", "expires_in_ms": 30000})
        );
        say(&first, r#"{"event":"begun","at_us":5,"id":7}"#);
        until_begun(&lamp, "c_1", 7);
        // Answered: its conversation may go on, so its id stays.
        say(&first, r#"{"outcome":"spoken"}"#);
        until_told(&told, 1);
        assert_eq!(begun_id(&lamp, "c_1"), Some(7));
        // The next start names it; another computer's names nothing.
        lamp.act("c_1", 4, &start(4), 0).unwrap();
        let (second, line) = accepted(&listener);
        assert_eq!(
            line,
            json!({"skill": "control", "turn": "start", "expires_in_ms": 30000, "after": 7})
        );
        lamp.act("c_2", 4, &start(4), 0).unwrap();
        let (other, line) = accepted(&listener);
        assert!(line.get("after").is_none(), "{line}");
        say(
            &other,
            r#"{"outcome":"refused","why":"a conversation is going on"}"#,
        );
        // Refused before its begun: the earlier id stays.
        say(
            &second,
            r#"{"outcome":"refused","why":"a conversation is going on"}"#,
        );
        until_told(&told, 3);
        assert_eq!(begun_id(&lamp, "c_1"), Some(7));
        // So its end still ends the earlier turn's conversation.
        let end = op(r#"{"id":5,"op":"turn.end"}"#);
        let ending = later(lamp.reply("c_1", 5, &end, 0));
        let (asked, line) = accepted(&listener);
        assert_eq!(line, json!({"skill": "control", "turn": "end", "id": 7}));
        say(&asked, r#"{"outcome":"ended"}"#);
        assert_eq!(
            ending.join().unwrap().unwrap(),
            json!({"turn": "ended", "started": 3})
        );
        // Used: forgotten.
        assert_eq!(begun_id(&lamp, "c_1"), None);
        // A start begun replaces the earlier id, whatever its outcome.
        lamp.act("c_1", 6, &start(6), 0).unwrap();
        let (third, _) = accepted(&listener);
        say(&third, r#"{"event":"begun","at_us":5,"id":9}"#);
        until_begun(&lamp, "c_1", 9);
        say(&third, r#"{"outcome":"spoken"}"#);
        until_told(&told, 4);
        lamp.act("c_1", 7, &start(7), 0).unwrap();
        let (fourth, line) = accepted(&listener);
        assert_eq!(line["after"], json!(9));
        say(&fourth, r#"{"event":"begun","at_us":6,"id":10}"#);
        until_begun(&lamp, "c_1", 10);
        // An end names the newest; the voice saying it is no turn of this
        // computer's forgets it too.
        let ending = later(lamp.reply("c_1", 8, &end, 0));
        let (asked, line) = accepted(&listener);
        assert_eq!(line, json!({"skill": "control", "turn": "end", "id": 10}));
        say(
            &asked,
            r#"{"outcome":"refused","why":"not this computer's turn"}"#,
        );
        let refused = ending.join().unwrap().unwrap_err();
        assert_eq!(
            (refused.code, refused.why.as_str()),
            (ErrorCode::Busy, NOT_THIS_TURN)
        );
        assert_eq!(begun_id(&lamp, "c_1"), None);
        say(&fourth, r#"{"outcome":"spoken"}"#);
        until_told(&told, 5);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The voice's refusals of an end or a mute, in the protocol's words.
    #[test]
    fn the_voices_refusals_are_the_protocols() {
        let refused = |why: &str| json!({"outcome": "refused", "why": why});
        let busy = refusal_of(&refused(NOT_THIS_TURN), NO_TURNS);
        assert_eq!(
            (busy.code, busy.why.as_str()),
            (ErrorCode::Busy, NOT_THIS_TURN)
        );
        let older = refusal_of(&refused("not a say request"), NO_TURNS);
        assert_eq!(
            (older.code, older.why.as_str()),
            (ErrorCode::Refused, NO_TURNS)
        );
        let older = refusal_of(&refused("not a say request"), NO_MUTE);
        assert_eq!(older.why, NO_MUTE);
        let muted = refusal_of(&refused("the lamp is muted"), NO_TURNS);
        assert_eq!(
            (muted.code, muted.why.as_str()),
            (ErrorCode::Refused, "the lamp is muted")
        );
        assert_eq!(
            refusal_of(&json!({"outcome": "expired"}), NO_TURNS).why,
            "expired"
        );
    }

    /// docs/control.md, "Turns and listening": a computer's mute is one say
    /// socket connection held for it, `listen: false` on it; `true` is said
    /// on that same connection and it closes; the computer gone closes it
    /// (a `stop` doesn't); a hand on the head lifting it is the computer's
    /// `done`; an older voice's refusal is said as that.
    #[test]
    fn a_computers_mute_is_one_held_connection_let_go_as_it_goes() {
        let dir = dir("mute");
        let path = dir.join("say.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let (events, told) = kept();
        let mut lamp = Lamp::new(None, None, Some(path), "boot".into(), events);
        let off = op(r#"{"id":5,"op":"listen.set","on":false}"#);
        let on = op(r#"{"id":6,"op":"listen.set","on":true}"#);
        let closed = |stream: &UnixStream| {
            let mut rest = String::new();
            BufReader::new(stream).read_line(&mut rest).unwrap() == 0
        };
        // Nothing held: said so, the voice not asked.
        assert_eq!(
            now(lamp.reply("c_1", 6, &on, 0)),
            json!({"mute": "nothing_held"})
        );
        untouched(&listener);
        let taking = later(lamp.reply("c_1", 5, &off, 0));
        let (held, line) = accepted(&listener);
        assert_eq!(line, json!({"skill": "control", "listen": false}));
        // While the voice takes it, another is busy.
        let busy = lamp.reply("c_1", 6, &on, 0).unwrap_err();
        assert_eq!(
            (busy.code, busy.why.as_str()),
            (ErrorCode::Busy, MUTE_BEING_TAKEN)
        );
        say(&held, r#"{"outcome":"muted"}"#);
        assert_eq!(taking.join().unwrap().unwrap(), json!({"mute": "held"}));
        // Again while held: nothing new.
        assert_eq!(now(lamp.reply("c_1", 7, &off, 0)), json!({"mute": "held"}));
        untouched(&listener);
        // Lifted on its own connection, which then closes.
        assert_eq!(now(lamp.reply("c_1", 6, &on, 0)), json!({"mute": "lifted"}));
        let mut line = String::new();
        BufReader::new(&held).read_line(&mut line).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&line).unwrap(),
            json!({"skill": "control", "listen": true})
        );
        assert!(closed(&held));

        // Held again; a stop leaves it, the computer gone closes it.
        let taking = later(lamp.reply("c_1", 8, &off, 0));
        let (held, _) = accepted(&listener);
        say(&held, r#"{"outcome":"muted"}"#);
        taking.join().unwrap().unwrap();
        lamp.stop("c_1", 0);
        held.set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let mut nothing = String::new();
        assert!(BufReader::new(&held).read_line(&mut nothing).is_err());
        held.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        lamp.gone("c_1", 0);
        assert!(closed(&held));
        assert_eq!(
            now(lamp.reply("c_1", 6, &on, 0)),
            json!({"mute": "nothing_held"})
        );

        // Gone while the voice takes it: let go as soon as it's taken.
        let taking = later(lamp.reply("c_1", 9, &off, 0));
        let (held, _) = accepted(&listener);
        lamp.gone("c_1", 0);
        say(&held, r#"{"outcome":"muted"}"#);
        assert_eq!(taking.join().unwrap().unwrap_err().code, ErrorCode::Refused);
        assert!(closed(&held));

        // A hand on the head lifts it: the computer is told, it's forgotten.
        let taking = later(lamp.reply("c_1", 10, &off, 0));
        let (held, _) = accepted(&listener);
        say(&held, r#"{"outcome":"muted"}"#);
        taking.join().unwrap().unwrap();
        say(&held, r#"{"outcome":"lifted","why":"a hand on the head"}"#);
        drop(held);
        until_told(&told, 1);
        assert_eq!(
            told.lock().unwrap()[0],
            (
                "c_1".to_owned(),
                json!({"ev": "done", "re": 10, "how": "cut_short", "why": "a hand on the head"})
            )
        );
        assert_eq!(
            now(lamp.reply("c_1", 6, &on, 0)),
            json!({"mute": "nothing_held"})
        );

        // A voice from before a computer's mute.
        let taking = later(lamp.reply("c_1", 11, &off, 0));
        let (held, _) = accepted(&listener);
        say(&held, r#"{"outcome":"refused","why":"not a say request"}"#);
        let older = taking.join().unwrap().unwrap_err();
        assert_eq!(
            (older.code, older.why.as_str()),
            (ErrorCode::Refused, NO_MUTE)
        );
        assert_eq!(
            now(lamp.reply("c_1", 6, &on, 0)),
            json!({"mute": "nothing_held"})
        );
        assert_eq!(told.lock().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// lamp-75c9, 9 Nov: a `turn.end` sent while the computer's own
    /// `turn.start` still waits for the voice ends that start. An earlier
    /// turn's id, kept past its end, is never sent in its place (the voice
    /// refused it "not this computer's turn"). Second's P2 on #5870: the
    /// door can't tell a start still waiting from one whose prompt the voice
    /// is saying, so it cancels the start on its own connection and answers
    /// only once the voice said what became of it: let go while it waited
    /// (the door's words), ended while its prompt was said (the voice's),
    /// and refused if it had already finished. A later `turn.end` never
    /// takes its place in a begun turn's.
    #[test]
    fn a_turn_end_ends_its_own_start_still_waiting() {
        let dir = dir("turn-end-waiting");
        let path = dir.join("say.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let (events, told) = kept();
        let mut lamp = Lamp::new(None, None, Some(path), "boot".into(), events);
        let end = op(r#"{"id":5,"op":"turn.end"}"#);
        // A first turn begins and ends: its id stays, as for a conversation.
        lamp.act("c_1", 2, &op(r#"{"id":2,"op":"turn.start"}"#), 0)
            .unwrap();
        let (first, _) = accepted(&listener);
        say(&first, r#"{"event":"begun","at_us":5,"id":7}"#);
        for _ in 0..300 {
            if begun_id(&lamp, "c_1") == Some(7) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        say(&first, r#"{"outcome":"dropped","why":"not answered"}"#);
        until_told(&told, 1);
        // Each case: a start, `turn.end` while the door hasn't seen it
        // begin, its cancel read at the voice, and the voice's answer.
        let mut case = |re: u64, voice: &str| {
            lamp.act(
                "c_1",
                re,
                &op(&format!(r#"{{"id":{re},"op":"turn.start"}}"#)),
                0,
            )
            .unwrap();
            let (start, _) = accepted(&listener);
            std::thread::sleep(Duration::from_millis(50));
            let answer = later(lamp.reply("c_1", 5, &end, 0));
            let mut cancel = String::new();
            BufReader::new(&start).read_line(&mut cancel).unwrap();
            assert_eq!(cancel.trim_end(), SAY_CANCEL);
            // Not answered from the door's own view: it waits for the voice.
            std::thread::sleep(Duration::from_millis(50));
            assert!(!answer.is_finished(), "answered before the voice said");
            say(&start, voice);
            untouched(&listener);
            answer.join().unwrap()
        };
        // Still waiting at the voice: let go, in the door's words.
        assert_eq!(
            case(3, r#"{"outcome":"dropped","why":"stopped by its asker"}"#).unwrap(),
            json!({"turn": "ended", "started": 3})
        );
        until_told(&told, 2);
        assert_eq!(
            told.lock().unwrap()[1],
            (
                "c_1".to_owned(),
                json!({"ev": "done", "re": 3, "how": "dropped", "why": ENDED_BEFORE_BEGUN})
            )
        );
        // Its prompt being said: the voice ended it, in its words.
        assert_eq!(
            case(4, r#"{"outcome":"dropped","why":"ended by the computer"}"#).unwrap(),
            json!({"turn": "ended", "started": 4})
        );
        until_told(&told, 3);
        assert_eq!(
            told.lock().unwrap()[2],
            (
                "c_1".to_owned(),
                json!({"ev": "done", "re": 4, "how": "dropped", "why": "dropped: ended by the computer"})
            )
        );
        // Already finished: not ended by this `turn.end`, and said so.
        let finished = case(6, r#"{"outcome":"spoken"}"#).unwrap_err();
        assert!(
            finished.why.contains("already finished"),
            "{}",
            finished.why
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Second's native reproduction on #5870, with the voice's own say
    /// socket and turns: a `turn.end` while the start's prompt is said, its
    /// `begun` not yet come (the window opens only once the prompt is
    /// heard whole). Before, the door answered "ended" from its own view
    /// while the voice kept the turn: the prompt's end then opened its
    /// window and the person's next line was taken as the turn's. Now the
    /// voice ends it, cuts its prompt, and the door answers only then.
    #[test]
    fn a_turn_end_while_its_prompt_is_said_ends_the_turn_at_the_voice() {
        use lamp_voice::control_turn::{ControlTurns, ENDED_BY_COMPUTER};
        use lamp_voice::say::{Outcome, Recordings, Says};
        let now = lamp_ipc::monotonic_us;
        let dir = dir("turn-end-prompt");
        let path = dir.join("say.sock");
        let mut says = Says::serve(&path, None, &dir, Recordings::none()).unwrap();
        let (events, told) = kept();
        let mut lamp = Lamp::new(None, None, Some(path), "boot".into(), events);
        lamp.act(
            "c_1",
            3,
            &op(r#"{"id":3,"op":"turn.start","prompt":"Tell me one thing."}"#),
            0,
        )
        .unwrap();
        // The voice's loop takes it and begins saying its prompt.
        let ticket = (0..300)
            .find_map(|_| {
                says.poll(now());
                let taken = says.take_turn_start(now());
                if taken.is_none() {
                    std::thread::sleep(Duration::from_millis(10));
                }
                taken
            })
            .map(|(prompt, _, ticket)| {
                assert_eq!(prompt.as_deref(), Some("Tell me one thing."));
                ticket
            })
            .expect("the voice took the start");
        let mut turns = ControlTurns::default();
        let said = turns.prompt(ticket);
        said.begun(now());
        turns.tick(now(), None, false, true);
        assert!(turns.going_on());
        // Answered now or later, the voice's view is what is checked.
        let answer = match lamp.reply("c_1", 5, &op(r#"{"id":5,"op":"turn.end"}"#), 0) {
            Ok(Reply::Now(with)) => std::thread::spawn(move || Ok(with)),
            reply => later(reply),
        };
        // The cancel reaches the voice on the start's connection.
        for _ in 0..300 {
            says.poll(now());
            if said.stopped_by_asker() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(said.stopped_by_asker(), "its prompt is cut with it");
        // In the coordinator's order, a line offered before the pass's tick
        // (Second's P2 at e1db6ae37e): a cancelled turn takes none.
        assert!(
            !turns.admitted(1, now()),
            "a line was taken as the cancelled turn's"
        );
        turns.tick(now(), None, false, true);
        assert!(!turns.going_on(), "the voice still has the turn");
        assert_eq!(
            answer.join().unwrap().unwrap(),
            json!({"turn": "ended", "started": 3})
        );
        // The prompt's own end opens no window; the person's line is theirs.
        said.answer(Outcome::Dropped("cut".into()));
        turns.tick(now(), None, false, true);
        assert!(!turns.admitted(2, now()));
        until_told(&told, 1);
        assert_eq!(
            told.lock().unwrap()[0],
            (
                "c_1".to_owned(),
                json!({"ev": "done", "re": 3, "how": "dropped", "why": format!("dropped: {ENDED_BY_COMPUTER}")})
            )
        );
        drop(says);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// docs/control.md, "Turns and listening": a computer gone (its last
    /// connection, a revoke) with a `turn.start` still waiting for the
    /// voice has that start's connection shut at once, so the voice drops
    /// it and gives no cue; another computer's waiting start, and a begun
    /// turn of the computer's, are left as they were.
    #[test]
    fn a_waiting_turn_start_is_shut_when_its_computer_goes() {
        let dir = dir("turn-gone");
        let path = dir.join("say.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let (events, _told) = kept();
        let mut lamp = Lamp::new(None, None, Some(path), "boot".into(), events);
        let start = op(r#"{"id":3,"op":"turn.start"}"#);
        let open_for = |stream: &UnixStream, within: Duration| {
            stream.set_read_timeout(Some(within)).unwrap();
            let mut rest = String::new();
            let read = BufReader::new(stream).read_line(&mut rest);
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            read.is_err()
        };
        lamp.act("c_1", 3, &start, 0).unwrap();
        let (going, _) = accepted(&listener);
        lamp.act("c_2", 3, &start, 0).unwrap();
        let (staying, _) = accepted(&listener);
        // Let the door's threads keep both connections.
        std::thread::sleep(Duration::from_millis(50));
        let asked = std::time::Instant::now();
        lamp.gone("c_1", 0);
        let mut rest = String::new();
        assert_eq!(BufReader::new(&going).read_line(&mut rest).unwrap(), 0);
        assert!(
            asked.elapsed() < Duration::from_secs(1),
            "{:?}",
            asked.elapsed()
        );
        assert!(open_for(&staying, Duration::from_millis(200)));
        // Begun, c_2's turn is no longer waiting: its going leaves it.
        say(&staying, r#"{"event":"begun","at_us":5,"id":7}"#);
        for _ in 0..300 {
            if begun_id(&lamp, "c_2") == Some(7) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(begun_id(&lamp, "c_2"), Some(7));
        lamp.gone("c_2", 0);
        assert!(open_for(&staying, Duration::from_millis(200)));
        say(&staying, r#"{"outcome":"spoken"}"#);
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn a_pending_cancel_without_a_voice_outcome_is_not_confirmed_ended() {
        let dir = dir("end-no-outcome");
        let path = dir.join("say.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let (events, _told) = kept();
        let mut lamp = Lamp::new(None, None, Some(path), "boot".into(), events);
        lamp.act(
            "c_1",
            3,
            &op(r#"{"id":3,"op":"turn.start","prompt":"Synthetic prompt."}"#),
            0,
        )
        .unwrap();
        let (start, _) = accepted(&listener);
        let end = op(r#"{"id":5,"op":"turn.end"}"#);
        let answer = later(lamp.reply("c_1", 5, &end, 0));
        let mut cancel = String::new();
        BufReader::new(&start).read_line(&mut cancel).unwrap();
        assert_eq!(cancel.trim_end(), SAY_CANCEL);
        // The transport ends without any voice outcome: cancellation was sent,
        // but this connection supplied no authoritative completion receipt.
        start.shutdown(std::net::Shutdown::Write).unwrap();
        let result = answer.join().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(result.is_err(), "ended without a voice outcome: {result:?}");
    }

    #[test]
    fn pending_cancels_share_one_reply_deadline() {
        let dir = dir("end-shared-deadline");
        let path = dir.join("say.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let (events, _told) = kept();
        let mut lamp = Lamp::new(None, None, Some(path), "boot".into(), events);
        lamp.act("c_1", 3, &op(r#"{"id":3,"op":"turn.start"}"#), 0)
            .unwrap();
        let (first, _) = accepted(&listener);
        lamp.act("c_1", 4, &op(r#"{"id":4,"op":"turn.start"}"#), 0)
            .unwrap();
        let (second, _) = accepted(&listener);
        let end = op(r#"{"id":5,"op":"turn.end"}"#);
        let began = std::time::Instant::now();
        let answer = later(lamp.reply("c_1", 5, &end, 0));
        for start in [&first, &second] {
            let mut cancel = String::new();
            BufReader::new(start).read_line(&mut cancel).unwrap();
            assert_eq!(cancel.trim_end(), SAY_CANCEL);
        }
        // Each gap is below two seconds, but the second acknowledgment is
        // outside the single two-second budget advertised for turn.end.
        let peers = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1300));
            say(
                &first,
                r#"{"outcome":"dropped","why":"stopped by its asker"}"#,
            );
            std::thread::sleep(Duration::from_millis(1000));
            say(
                &second,
                r#"{"outcome":"dropped","why":"stopped by its asker"}"#,
            );
        });
        let result = answer.join().unwrap();
        let took = began.elapsed();
        peers.join().unwrap();
        eprintln!("two pending cancels: reply returned after {took:?}");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            result.is_err(),
            "accepted beyond shared deadline after {took:?}: {result:?}"
        );
    }
    /// A slow voice must not keep the door's lock while its cancel is sent.
    /// Fill the real Unix socket's send buffer, then time reply dispatch only.
    #[test]
    fn a_pending_cancel_write_is_off_the_door_lock() {
        for sample in 0..3 {
            let dir = dir(&format!("cancel-write-{sample}"));
            let path = dir.join("say.sock");
            let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
            let (events, told) = kept();
            let mut lamp = Lamp::new(None, None, Some(path), "boot".into(), events);
            lamp.act("c_1", 3, &op(r#"{"id":3,"op":"turn.start"}"#), 0)
                .unwrap();
            let (peer, _) = accepted(&listener);
            let ready_by = std::time::Instant::now() + Duration::from_secs(2);
            let socket = loop {
                let waiting = lamp
                    .voice
                    .lock()
                    .unwrap()
                    .turns
                    .get("c_1")
                    .and_then(|turns| turns.first())
                    .and_then(|turn| turn.waiting.as_ref())
                    .map(|stream| stream.try_clone().unwrap());
                if let Some(socket) = waiting {
                    break socket;
                }
                assert!(std::time::Instant::now() < ready_by, "start not registered");
                std::thread::yield_now();
            };
            socket
                .set_write_timeout(Some(Duration::from_millis(1)))
                .unwrap();
            let mut blocked = false;
            for _ in 0..256 {
                match (&socket).write(&[b'x'; 64 * 1024]) {
                    Ok(n) => assert!(n > 0),
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) =>
                    {
                        blocked = true;
                        break;
                    }
                    Err(error) => panic!("filling the fixture socket: {error}"),
                }
            }
            assert!(blocked, "fixture must establish write backpressure");
            let began = std::time::Instant::now();
            let reply = lamp.reply("c_1", 5, &op(r#"{"id":5,"op":"turn.end"}"#), 0);
            let dispatch = began.elapsed();
            let answer = later(reply);
            // Release the fixture before asserting, including on the old code.
            drop(peer);
            drop(socket);
            let result = answer.join().unwrap();
            until_told(&told, 1);
            let _ = std::fs::remove_dir_all(&dir);
            eprintln!("cancel dispatch with write backpressure {sample}: {dispatch:?}");
            assert!(dispatch < Duration::from_millis(50), "{dispatch:?}");
            assert!(result.is_err(), "peer supplied no outcome: {result:?}");
        }
    }
    #[test]
    fn pending_cancel_receipts_require_explicit_outcomes_from_every_start() {
        for (name, first, second, refusal) in [
            (
                "dropped-refused",
                Some(r#"{"outcome":"dropped","why":"stopped by its asker"}"#),
                Some(r#"{"outcome":"refused","why":"private"}"#),
                None,
            ),
            (
                "expired-dropped",
                Some(r#"{"outcome":"expired"}"#),
                Some(r#"{"outcome":"dropped","why":"stopped by its asker"}"#),
                None,
            ),
            (
                "finished",
                Some(r#"{"outcome":"spoken"}"#),
                Some(r#"{"outcome":"expired"}"#),
                Some("already finished"),
            ),
            (
                "one-missing",
                Some(r#"{"outcome":"expired"}"#),
                None,
                Some("no outcome"),
            ),
            (
                "no-field",
                Some(r#"{}"#),
                Some(r#"{"outcome":"expired"}"#),
                Some("no outcome"),
            ),
            (
                "wrong-kind",
                Some(r#"{"outcome":"muted"}"#),
                Some(r#"{"outcome":"expired"}"#),
                Some("unexpected say outcome"),
            ),
            (
                "malformed",
                Some("not-json"),
                Some(r#"{"outcome":"expired"}"#),
                Some("no outcome"),
            ),
        ] {
            let dir = dir(&format!("cancel-receipts-{name}"));
            let path = dir.join("say.sock");
            let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
            let (events, told) = kept();
            let mut lamp = Lamp::new(None, None, Some(path), "boot".into(), events);
            let mut peers = Vec::new();
            for re in [3, 4] {
                lamp.act(
                    "c_1",
                    re,
                    &op(&format!(r#"{{"id":{re},"op":"turn.start"}}"#)),
                    0,
                )
                .unwrap();
                peers.push(accepted(&listener).0);
            }
            let began = Instant::now();
            let answer = later(lamp.reply("c_1", 5, &op(r#"{"id":5,"op":"turn.end"}"#), 0));
            for peer in &peers {
                let mut cancel = String::new();
                BufReader::new(peer).read_line(&mut cancel).unwrap();
                assert_eq!(cancel.trim_end(), SAY_CANCEL);
            }
            for (peer, outcome) in peers.into_iter().zip([first, second]) {
                if let Some(outcome) = outcome {
                    say(&peer, outcome);
                }
                drop(peer);
            }
            let result = answer.join().unwrap();
            let elapsed = began.elapsed();
            until_told(&told, 2);
            let _ = std::fs::remove_dir_all(&dir);
            eprintln!("pending cancel receipts {name}: reply after {elapsed:?}");
            match refusal {
                Some(why) => {
                    let error = result.unwrap_err();
                    assert_eq!(error.code, ErrorCode::Refused);
                    assert!(error.why.contains(why), "{name}: {}", error.why);
                }
                None => assert_eq!(result.unwrap(), json!({"turn":"ended","started":4})),
            }
        }
    }

    #[test]
    fn expired_pending_cancel_budget_half_closes_only_the_request_side() {
        use std::io::Read as _;
        let (client, mut peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        cancel_turn_before(&client, Instant::now());
        assert_eq!(
            peer.read(&mut [0u8; 1]).unwrap(),
            0,
            "the request side reaches EOF"
        );
        say(
            &peer,
            r#"{"outcome":"dropped","why":"stopped by its asker"}"#,
        );
        assert_eq!(
            outcome_with(BufReader::new(client), |_| {}),
            Ok(("dropped", Some("dropped: stopped by its asker".into()))),
            "the response side must remain readable"
        );
    }
}
