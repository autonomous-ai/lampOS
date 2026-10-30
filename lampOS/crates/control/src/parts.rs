//! Each action routed to the part of the lamp that already does it
//! (docs/control.md, "Routing"), held to that part's own limits:
//!
//! - `gesture` and `move` → the body's mood socket, as lamp-skills sends
//!   them (`lamp_skills::body_ask`), at `skill` priority with the source
//!   `control:<client>`. The body plays them with its own move check, reach,
//!   speed and anti-tip limits, or drops them with why in its journal.
//!   The body plays one at a time, so the door remembers when the last
//!   motion it sent ends, whoever asked, and refuses a `gesture`, `move` or
//!   `express` move asked before then as `busy`, with `retry_ms` (#4209):
//!   the drop the body would make, said to the computer. A length the door
//!   can't know (a move folder's gesture) holds nothing.
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
//! `look` at `face` and at `name` are not in this release, and refused as
//! that. `joints`' `yaw` and `pitch` go as the holder's look, the same
//! degrees from home; its `roll`, `shoulder` and `elbow` wait for the body's
//! goal stream. Not routed yet, each refused with what it waits for: `play`
//! and `sound` (a client's package,
//! with the Store's packaging). State and events from the
//! bus come in their own piece.

use std::collections::HashMap;
use std::io::{BufRead as _, BufReader, Write as _};
use std::net::Shutdown;
use std::os::unix::net::{UnixDatagram, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
/// A `listen.set` asked while this computer's last is still with the voice.
pub const MUTE_BEING_TAKEN: &str = "this computer's mute is still being taken by the voice";

/// Where the events of actions that end later go: the client, and the
/// event (`done`).
pub type Events = Arc<dyn Fn(&str, Value) + Send + Sync>;
/// Where a client's binary camera frames go (`crate::camera`'s fast stream).
pub type Binary = Arc<dyn Fn(&str, &[u8]) + Send + Sync>;

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

/// `look` at `face` and at `name`: out of this release.
pub const NOT_IN_THIS_RELEASE: &str = "not in this release: look at a face or a name";

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
    /// When the last motion sent to the body ends, on the door's clock.
    /// One for the lamp, since the body plays one at a time whoever asked
    /// (#4209). A `stop` doesn't free it: the door never stops the body,
    /// which plays the motion to its end.
    motion: Option<u64>,
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

/// A computer's turn start not yet answered.
struct Turn {
    seq: u64,
    /// Its say socket connection while the voice has not begun it: shut
    /// when the computer goes (`gone`), which drops it at the
    /// voice, so a computer gone or revoked gets no cue and no window.
    /// Let go once it began: a begun turn is ended by its `turn.end` or
    /// the person, as before.
    waiting: Option<UnixStream>,
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
            motion: None,
            camera: None,
            clips: None,
            voice: Arc::default(),
            actions: None,
            says: Says::default(),
            next_say: 0,
        }
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

    /// Refused as `busy` while the last motion sent still plays, with when
    /// it ends (#4209).
    fn free_for_motion(&self, now_us: u64) -> Result<(), Error> {
        match &self.motion {
            Some(until_us) if *until_us > now_us => {
                let left_ms = (until_us - now_us).div_ceil(1_000);
                let mut error = Error::new(
                    ErrorCode::Busy,
                    format!("busy: a motion plays for {left_ms} ms more"),
                );
                error.retry_ms = Some(left_ms);
                Err(error)
            }
            _ => Ok(()),
        }
    }

    /// Sends a motion to the body when none still plays, and holds the
    /// slot for its length, if known; a refused one moves nothing.
    fn motion(
        &mut self,
        ask: Value,
        played_us: Option<u64>,
        client: &str,
        now_us: u64,
    ) -> Result<Value, Error> {
        self.free_for_motion(now_us)?;
        let sent = self.to_body(ask, client)?;
        self.motion = played_us.map(|played_us| now_us + played_us);
        Ok(sent)
    }

    fn express(
        &mut self,
        client: &str,
        feeling: &str,
        strength: f32,
        now_us: u64,
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
                let played_us = played_us(scaled.step_ms.into(), scaled.frames.len());
                self.motion(json!({"move": scaled}), Some(played_us), client, now_us)?;
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
            // Out of this release, said as that (the local leader, 29 Oct).
            LookAt::Target(LookTarget::Face | LookTarget::Name) => {
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
            })
            .map_err(|_| not_here("the body's lease"))?;
        if let Some(follow) = &self.follow {
            *follow.lock().expect("follower") = None;
        }
        Ok(json!({"to": "body"}))
    }

    /// `joints`: the head's `yaw` and `pitch`, in degrees from home, go as
    /// the holder's look on the body's lease socket (the same degrees, cut
    /// to the same reach; its `done` says where the head got to). A joint
    /// not named stays where it is now, as `get` `body` says it. `roll`,
    /// `shoulder` and `elbow` wait for the body's own goal stream.
    fn joints(
        &self,
        client: &str,
        re: u64,
        targets: &crate::protocol::Joints,
    ) -> Result<Value, Error> {
        if targets.roll.is_some() || targets.shoulder.is_some() || targets.elbow.is_some() {
            return Err(waits_for(
                "the body's goal stream for roll, shoulder and elbow; yaw and pitch go now",
            ));
        }
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
                "name both yaw and pitch until the body has said where it is (get body)",
            ));
        };
        self.look(client, re, &LookAt::Angles(Angles { yaw, pitch }))
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
                .push(Turn { seq, waiting: None });
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
                    }
                }
            };
            let (how, why) = match UnixStream::connect(&path)
                .and_then(|mut stream| writeln!(stream, "{line}").map(|()| stream))
            {
                Ok(stream) => {
                    // Kept with the start while it waits; gone meanwhile, its
                    // computer's start is let go at once.
                    let kept = stream.try_clone().ok();
                    let waits = match voice.lock().expect("voice").turn(&client, seq) {
                        Some(turn) => {
                            turn.waiting = kept;
                            true
                        }
                        None => false,
                    };
                    if !waits {
                        let _ = stream.shutdown(Shutdown::Both);
                    }
                    let _ = stream.set_read_timeout(Some(SAY_ANSWER_WITHIN));
                    outcome_with(BufReader::new(stream), begun)
                }
                Err(error) => (
                    "dropped",
                    Some(format!("the voice did not take it: {error}")),
                ),
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
            {
                let mut voice = voice.lock().expect("voice");
                if let Some(turns) = voice.turns.get_mut(&client) {
                    turns.retain(|turn| turn.seq != seq);
                    if turns.is_empty() {
                        voice.turns.remove(&client);
                    }
                }
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
        let begun = {
            let voice = self.voice.lock().expect("voice");
            match voice.begun.get(client) {
                Some(begun) => *begun,
                // Asked and not begun: the voice has no id for it yet.
                None if voice
                    .turns
                    .get(client)
                    .is_some_and(|turns| !turns.is_empty()) =>
                {
                    return Err(Error::new(
                        ErrorCode::Busy,
                        "this computer's turn has not begun yet",
                    ));
                }
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

/// The say socket's outcome, past the `begun` event, in the protocol's
/// words (`done.how`: `finished`, `cut_short`, `dropped`; the Reviewer-lead's
/// and harness-reviewer's P2 on #3805): `spoken` is `finished`; `dropped`,
/// `expired`, `refused` and no outcome at all are `dropped`, the voice's
/// own word and why in `why`.
fn outcome(reader: impl std::io::BufRead) -> (&'static str, Option<String>) {
    outcome_with(reader, |_| {})
}

/// As [`outcome`], each event before it (`begun`) handed to `event`.
fn outcome_with(
    reader: impl std::io::BufRead,
    mut event: impl FnMut(&Value),
) -> (&'static str, Option<String>) {
    for line in reader.lines() {
        let Ok(line) = line else { break };
        let Ok(said) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if said.get("event").is_some() {
            event(&said);
            continue;
        }
        let word = said["outcome"].as_str().unwrap_or("failed");
        if word == "spoken" {
            return ("finished", None);
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
            return ("cut_short", Some(why.to_owned()));
        }
        let why = match said["why"].as_str() {
            Some(why) => format!("{word}: {why}"),
            None => word.to_owned(),
        };
        return ("dropped", Some(why));
    }
    ("dropped", Some("the voice gave no outcome".to_owned()))
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

/// How long the body plays a move of these frames: from the first frame
/// to the last, as its own clips count it (`lamp_body::clips::Clip::duration_us`).
fn played_us(step_ms: u64, frames: usize) -> u64 {
    (frames.saturating_sub(1) as u64) * step_ms * 1_000
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
    fn act(&mut self, client: &str, re: u64, op: &Op, now_us: u64) -> Result<Value, Error> {
        match op {
            // A built-in clip's own length; a move folder's gesture, which
            // the door can't see, holds nothing.
            Op::Gesture { name } => self.motion(
                json!({"gesture": name}),
                lamp_body::clips::named(name).map(lamp_body::clips::Clip::duration_us),
                client,
                now_us,
            ),
            Op::Move { step_ms, frames } => self.motion(
                json!({"move": {"name": "control", "step_ms": step_ms, "frames": frames}}),
                Some(played_us((*step_ms).into(), frames.len())),
                client,
                now_us,
            ),
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
            Op::Express { feeling, strength } => self.express(client, feeling, *strength, now_us),
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

    fn state(&self, kind: StateKind) -> Value {
        self.watch
            .as_ref()
            .and_then(|watch| watch.lock().ok().map(|watch| watch.state(kind)))
            .unwrap_or(Value::Null)
    }

    fn camera_streaming(&self, client: &str) -> bool {
        self.camera
            .as_ref()
            .is_some_and(|camera| camera.streaming(client))
    }

    fn camera_end(&mut self, client: &str) {
        if let Some(camera) = &mut self.camera {
            camera.end(client);
        }
    }

    fn stop(&mut self, client: &str, _: u64) {
        if let Some(camera) = &mut self.camera {
            camera.end(client);
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
            json!({"gesture": "nod", "source": "control:c_1", "priority": "skill"})
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

    fn sent(body: &UnixDatagram) -> usize {
        let mut buffer = [0u8; 16 * 1024];
        std::iter::from_fn(|| body.recv(&mut buffer).ok()).count()
    }

    const NOD: &str = r#"{"id":1,"op":"gesture","name":"nod"}"#;

    /// #4209: a motion asked while the last one sent still plays is refused
    /// `busy` with when to try again, whoever asks, never sent, and moves
    /// nothing; once it has played, the next is taken.
    #[test]
    fn a_motion_asked_while_the_last_still_plays_is_busy_until_it_ends() {
        let (mut lamp, body, dir) = with_body("busy");
        let nod_us = lamp_body::clips::NOD.duration_us();
        lamp.act("c_1", 1, &op(NOD), 1_000_000).unwrap();
        assert_eq!(sent(&body), 1);
        let error = lamp.act("c_2", 2, &op(NOD), 1_100_000).unwrap_err();
        assert_eq!(error.code, ErrorCode::Busy);
        let left_ms = (nod_us - 100_000) / 1_000;
        assert!(left_ms > 0);
        assert_eq!(error.retry_ms, Some(left_ms));
        assert_eq!(
            error.why,
            format!("busy: a motion plays for {left_ms} ms more")
        );
        assert_eq!(sent(&body), 0, "a refused motion isn't sent");
        // Refused again just before its end: still the first's end, not
        // pushed on by the refusals.
        let error = lamp
            .act("c_1", 3, &op(NOD), 1_000_000 + nod_us - 1_000)
            .unwrap_err();
        assert_eq!(error.retry_ms, Some(1));
        lamp.act("c_2", 4, &op(NOD), 1_000_000 + nod_us).unwrap();
        assert_eq!(sent(&body), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A move holds the slot for its frames at its step, as the body plays
    /// it: from the first frame to the last; `express` for its own move.
    #[test]
    fn a_moves_length_is_its_frames_at_its_step() {
        let (mut lamp, body, dir) = with_body("busy-move");
        let walk = op(
            r#"{"id":1,"op":"move","step_ms":500,"frames":[[0,0,0,0,0],[20,0,30,0,0],[20,0,30,0,0],[0,0,0,0,0]]}"#,
        );
        lamp.act("c_1", 1, &walk, 0).unwrap();
        let error = lamp.act("c_1", 2, &walk, 1_499_000).unwrap_err();
        assert_eq!(error.retry_ms, Some(1));
        lamp.act("c_1", 3, &walk, 1_500_000).unwrap();
        assert_eq!(sent(&body), 2);
        let glad = op(r#"{"id":4,"op":"express","feeling":"glad"}"#);
        lamp.act("c_1", 4, &glad, 3_000_000).unwrap();
        let written = crate::feelings::named("glad").unwrap().written();
        let glad_us = (written.frames.len() as u64 - 1) * u64::from(written.step_ms) * 1_000;
        let error = lamp.act("c_1", 5, &walk, 3_000_000).unwrap_err();
        assert_eq!(error.retry_ms, Some(glad_us / 1_000));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A `stop`, the asker's own or another's, leaves the slot: the door
    /// never stops the body, which plays the motion to its end.
    #[test]
    fn a_stop_leaves_the_motion_slot() {
        let (mut lamp, body, dir) = with_body("busy-stop");
        lamp.act("c_1", 1, &op(NOD), 0).unwrap();
        for client in ["c_2", "c_1"] {
            lamp.stop(client, 100_000);
            assert_eq!(
                lamp.act(client, 2, &op(NOD), 100_000).unwrap_err().code,
                ErrorCode::Busy
            );
        }
        assert_eq!(sent(&body), 1);
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
    /// roll and the arm wait for the body's goal stream.
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
            json!({"look": {"yaw": -10.0, "pitch": 5.0}})
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
            json!({"look": {"yaw": 4.0, "pitch": 3.0}})
        );
        let error = lamp
            .act(
                "c_1",
                15,
                &joints(
                    r#"{"id":15,"op":"joints","lease":"l_1","targets":{"roll":3},"within_ms":300}"#,
                ),
                0,
            )
            .unwrap_err();
        assert!(
            error.why.contains("roll, shoulder and elbow"),
            "{}",
            error.why
        );
    }

    #[test]
    fn what_is_not_routed_yet_says_what_it_waits_for() {
        let mut lamp = Lamp::new(None, None, None, "boot".into(), quiet());
        for (frame, waits) in [
            (
                r#"{"id":1,"op":"look","at":"ahead"}"#,
                "lease socket is not on",
            ),
            (r#"{"id":1,"op":"look","at":"face"}"#, NOT_IN_THIS_RELEASE),
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
        // Asked and not begun: no id to end it by yet.
        let early = lamp.reply("c_1", 4, &end, 0).unwrap_err();
        assert_eq!(early.code, ErrorCode::Busy);
        assert_eq!(early.why, "this computer's turn has not begun yet");
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
}
