//! What the voice says on the lamp's bus (`docs/bus.md`): what it is doing,
//! what it heard and what it said.
//!
//! The bus never holds a publisher up: a send is non-blocking, and one that
//! nobody takes costs nothing. So this is called from the coordinator's own
//! loop, and nothing here can fail the voice: a bus that cannot be used is
//! said once at start, and after that every error is dropped.
//!
//! Three topics, with the types of `lamp-msgs`:
//!
//! - `voice/state`: resting, listening, thinking or speaking. When it
//!   changes, and once a second so that a subscriber can tell a voice that
//!   is resting from one that has gone.
//! - `voice/heard`: the provider's text of what a person said, in pieces as
//!   it comes, with the number of the turn whose input began last before
//!   it. Between things said the voice says once a second that it is still
//!   there, on this topic and the next.
//! - `voice/said`: the provider's text of what the lamp said, with the
//!   number of the request it answered.
//!
//! A fourth, `room/state` (quiet, busy or not known), comes from the
//! provider worker, which is where the room is judged
//! ([`crate::room_told`]): [`RoomOut`], given the bus's root by a flag,
//! since a worker is started with no environment.
//!
//! The text is what the provider wrote down, which is not always what was
//! said. It stays on this machine: the bus's root is a directory in memory
//! that only the lamp's own programs can enter.
use crate::ring_wire::RingPhase;
use lamp_bus::{Bus, Publisher};
use lamp_msgs::{
    LinkFault, Room, RoomState, Speech, TurnVerdict, Voice, VoiceLink, VoiceState, VoiceTurn,
};
use std::{
    io,
    path::{Path, PathBuf},
};

/// Where the coordinator reads whether to publish. `on` only. The root is
/// the bus's own to say (`LAMP_BUS_ROOT`, or `/run/lampos-bus`).
pub const ENVIRONMENT: &str = "LAMP_BUS";
/// `on`, with the bus on, for the voice to publish `voice/link` too: the
/// signal a watcher of the lamp's reachability uses (#830, `lamp/online`).
pub const LINK_ENVIRONMENT: &str = "LAMP_VOICE_LINK";
/// The name the voice publishes under.
pub const SOURCE: &str = "voice";
/// An unchanged state is said again this often.
const REPEAT_US: u64 = 1_000_000;

/// The provider worker's flag for the bus's root, followed by the path.
pub const ROOT_FLAG: &str = "--bus-root";

/// The provider worker's arguments for what the environment says: the
/// bus's root, if the voice publishes at all.
pub fn worker_arguments_from_environment() -> Vec<String> {
    worker_arguments(
        std::env::var(ENVIRONMENT).ok().as_deref(),
        std::env::var("LAMP_BUS_ROOT").ok().as_deref(),
    )
}

fn worker_arguments(switch: Option<&str>, root: Option<&str>) -> Vec<String> {
    if switch != Some("on") {
        return Vec::new();
    }
    let root = root
        .filter(|root| Path::new(root).is_absolute())
        .unwrap_or("/run/lampos-bus");
    vec![ROOT_FLAG.to_owned(), root.to_owned()]
}

/// Take a leading `--bus-root PATH` off a worker's flags.
pub fn take_root(flags: &[String]) -> io::Result<(Option<PathBuf>, &[String])> {
    match flags {
        [flag, root, rest @ ..] if flag == ROOT_FLAG => {
            if !Path::new(root).is_absolute() {
                return Err(io::Error::other("--bus-root must be absolute"));
            }
            Ok((Some(PathBuf::from(root)), rest))
        }
        [flag] if flag == ROOT_FLAG => Err(io::Error::other("--bus-root needs a path")),
        _ => Ok((None, flags)),
    }
}

/// How the room is, from the process that judges it.
pub struct RoomOut {
    room: Publisher,
    last: Option<(crate::room_told::Room, u64)>,
}

impl RoomOut {
    pub fn on(root: &Path) -> io::Result<Self> {
        let bus = Bus::new(root)?;
        Ok(Self {
            room: bus.publisher_declared(RoomState::TOPIC, &RoomState::declaration(), SOURCE)?,
            last: None,
        })
    }

    /// Call every tick: it publishes when the room changes, and once a
    /// second says it again. Said again and not only kept alive: a reader
    /// that acts on a quiet room (`docs/expression.md`) takes a judgement
    /// a few seconds old as no judgement, and the room is judged all the
    /// time whether or not it changes.
    pub fn tell(&mut self, room: crate::room_told::Room, now_us: u64) {
        if self
            .last
            .is_some_and(|(told, at)| told == room && now_us.saturating_sub(at) < REPEAT_US)
        {
            return;
        }
        self.last = Some((room, now_us));
        let state = match room {
            crate::room_told::Room::Unknown => Room::Unknown,
            crate::room_told::Room::Quiet => Room::Quiet,
            crate::room_told::Room::Busy => Room::Busy,
        };
        let _ = self.room.publish_json(now_us, &RoomState { state });
    }
}

/// Each turn's verdict (`voice/turn`, #1155), from the process that
/// reaches it: answered, or its answer held back as not said to the lamp.
/// The Policy's hello counts only turns to the lamp toward its quiet.
pub struct TurnOut {
    turn: Publisher,
}

impl TurnOut {
    pub fn on(root: &Path) -> io::Result<Self> {
        let bus = Bus::new(root)?;
        Ok(Self {
            turn: bus.publisher_declared(VoiceTurn::TOPIC, &VoiceTurn::declaration(), SOURCE)?,
        })
    }

    /// Says `request`'s verdict, once, as it is reached. A send that
    /// fails changes nothing for the voice.
    pub fn tell(&mut self, request: u64, verdict: TurnVerdict, now_us: u64) {
        let _ = self
            .turn
            .publish_json(now_us, &VoiceTurn { request, verdict });
    }
}

/// Where the coordinator reads whether what the microphone hears goes on
/// the bus. `on` only, and only with [`ENVIRONMENT`] on as well. Its own
/// switch, off unless someone set it: this is the one topic that carries
/// sound, and with it any program of the lamp's group can hear the room.
pub const AUDIO_ENVIRONMENT: &str = "LAMP_BUS_AUDIO";
/// The samples of one message: 20 ms at 16 kHz, as the topic promises.
const AUDIO_BLOCK_BYTES: usize = 640;

/// What the microphone hears, for a part of the lamp that listens for
/// itself (`mic1/audio` or `mic2/audio`, `lamp_msgs::Audio`): the sound as
/// the voice hears it, after its echo canceller, 16 kHz, one channel,
/// 20 ms to a message. Only what the voice was given: with the microphone
/// closed there is nothing to publish, and nothing is.
pub struct MicOut {
    audio: Publisher,
    held: Vec<u8>,
    sequence: u64,
    alive_at_us: u64,
}

impl MicOut {
    /// The publisher for the microphone called `microphone` (`mic1`,
    /// `mic2`), if both switches say `on` and the bus can be used. A bus
    /// that cannot is said once and the voice goes on without.
    pub fn from_environment(microphone: &str) -> Option<Self> {
        let on = |name| std::env::var(name).ok().as_deref() == Some("on");
        if !(on(ENVIRONMENT) && on(AUDIO_ENVIRONMENT)) {
            return None;
        }
        match Bus::from_environment().and_then(|bus| Self::on(&bus, microphone)) {
            Ok(out) => Some(out),
            Err(error) => {
                eprintln!(
                    "lamp-voice: the microphone's sound is not published on the bus: {error}"
                );
                None
            }
        }
    }

    pub fn on(bus: &Bus, microphone: &str) -> io::Result<Self> {
        let topic = lamp_msgs::Audio::TOPICS
            .into_iter()
            .find(|topic| topic.strip_suffix("/audio") == Some(microphone))
            .ok_or_else(|| io::Error::other("no audio topic for that microphone"))?;
        Ok(Self {
            audio: bus.publisher_declared(topic, &lamp_msgs::Audio::declaration(), SOURCE)?,
            held: Vec::with_capacity(AUDIO_BLOCK_BYTES * 2),
            sequence: 0,
            alive_at_us: 0,
        })
    }

    /// One block the voice was given, 10 ms of it. Every second block
    /// makes a message. Nothing waits for a listener, and a bus that
    /// fails costs the sound of that message and nothing else.
    pub fn block(&mut self, samples: &[i16], now_us: u64) {
        for sample in samples {
            self.held.extend_from_slice(&sample.to_le_bytes());
        }
        while self.held.len() >= AUDIO_BLOCK_BYTES {
            let header = lamp_msgs::Audio {
                sample_rate: 16_000,
                channels: 1,
                sample_format: lamp_msgs::Audio::S16LE.into(),
                sequence: self.sequence,
            };
            self.sequence += 1;
            if let Ok(payload) = header.block(&self.held[..AUDIO_BLOCK_BYTES]) {
                let _ = self
                    .audio
                    .publish_bytes(now_us, lamp_msgs::Audio::ENCODING, &payload);
            }
            self.held.drain(..AUDIO_BLOCK_BYTES);
            self.alive_at_us = now_us;
        }
    }

    /// Call every tick. With the microphone closed nothing is published,
    /// and this is what says the voice is still there.
    pub fn tick(&mut self, now_us: u64) {
        if now_us.saturating_sub(self.alive_at_us) >= 1_000_000 {
            self.alive_at_us = now_us;
            let _ = self.audio.alive();
        }
    }
}

/// With `LAMP_STOP_HEARD=on` (and the bus), a stop word heard by
/// `lamp-name`'s `stop.onnx` (`stop/heard`, #828) yields the lamp's reply
/// while it sounds, as a cut-in does, and the duplex rules then commit or
/// resume it. Off unless set.
pub const STOP_HEARD_ENVIRONMENT: &str = "LAMP_STOP_HEARD";

/// Whether the voice takes `stop/heard`.
pub fn stop_heard_on() -> bool {
    std::env::var(STOP_HEARD_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// The lamp's name, heard in the room's sound by the part that listens
/// for it (`name/heard`, `docs/name.md`): for the rule that decides
/// whether an answer is played (`crate::attention`).
pub struct NameIn {
    heard: lamp_bus::Subscriber,
    looked_at_us: Option<u64>,
}

/// The bus is looked at for a hearing this often. A hearing is said a
/// tenth of a second or more after the name began, and it is dated from
/// when it began, so this adds nothing that matters to the rule.
const NAME_LOOK_EVERY_US: u64 = 20_000;

impl NameIn {
    pub fn on(root: &Path) -> io::Result<Self> {
        Self::on_topic(root, lamp_msgs::NameHeard::TOPIC)
    }

    /// The same for `topic`: `name/heard`, each hearing when it is over, or
    /// `name/rising`, its first sign as the name is said (#782).
    pub fn on_topic(root: &Path, topic: &str) -> io::Result<Self> {
        let kind = if topic == lamp_msgs::StopHeard::TOPIC {
            lamp_msgs::StopHeard::TYPE
        } else {
            lamp_msgs::NameHeard::TYPE
        };
        Ok(Self {
            heard: Bus::new(root)?.subscribe_typed(topic, SOURCE, kind)?,
            looked_at_us: None,
        })
    }

    /// The subscriber, if the bus is switched on and can be used. A bus
    /// that cannot is said once and the voice goes on without.
    pub fn from_environment() -> Option<Self> {
        Self::from_environment_on(lamp_msgs::NameHeard::TOPIC)
    }

    /// The same for `topic`.
    pub fn from_environment_on(topic: &str) -> Option<Self> {
        if std::env::var(ENVIRONMENT).ok().as_deref() != Some("on") {
            return None;
        }
        let root = std::env::var_os(lamp_bus::ROOT_VARIABLE)
            .filter(|root| !root.is_empty())
            .unwrap_or_else(|| lamp_bus::DEFAULT_ROOT.into());
        match Self::on_topic(Path::new(&root), topic) {
            Ok(names) => Some(names),
            Err(error) => {
                eprintln!("lamp-voice: the lamp's name is not taken from the bus: {error}");
                None
            }
        }
    }

    /// When the newest hearing since the last look began, by the clock
    /// `now_us` is of, if there was one. Never waits, and looks at the
    /// bus at most every 20 ms however often it is called.
    pub fn newest(&mut self, now_us: u64) -> Option<u64> {
        self.newest_scored(now_us).map(|(began, _)| began)
    }

    /// The same, with the hearing's score.
    pub fn newest_scored(&mut self, now_us: u64) -> Option<(u64, f32)> {
        if self
            .looked_at_us
            .is_some_and(|at| now_us.saturating_sub(at) < NAME_LOOK_EVERY_US)
        {
            return None;
        }
        self.looked_at_us = Some(now_us);
        let mut newest: Option<(u64, f32)> = None;
        for _ in 0..8 {
            let Ok(Some(message)) = self.heard.recv(std::time::Duration::ZERO) else {
                break;
            };
            // A stop word's hearing (`stop/heard`) is the same score and
            // age as the name's.
            let heard = message
                .parse::<lamp_msgs::NameHeard>()
                .map(|heard| (heard.score, heard.ago_ms))
                .or_else(|_| {
                    message
                        .parse::<lamp_msgs::StopHeard>()
                        .map(|heard| (heard.score, heard.ago_ms))
                });
            if let Ok((score, ago_ms)) = heard {
                let began = now_us.saturating_sub(u64::from(ago_ms) * 1_000);
                if newest.is_none_or(|(before, _)| began >= before) {
                    newest = Some((began, score));
                }
            }
        }
        newest
    }
}

/// The lamp's head tapped, from `--touch` Controls (`controls/touch`, #711),
/// for the rule that decides whether an answer is played
/// (`crate::attention`): a tap asks the lamp to listen.
pub struct TapsIn {
    touches: lamp_bus::Subscriber,
    /// Where `voice/state` is read: whether the lamp was speaking.
    bus: Bus,
    looked_at_us: Option<u64>,
    /// The newest tap taken: its Controls session and event sequence.
    taken: Option<(String, u64)>,
    /// When this reader last saw the voice speaking.
    speaking_seen_us: Option<u64>,
}

/// A tap let go longer ago than this, by the time it is read, is not taken.
const TAP_FRESH_US: u64 = 2_000_000;

/// A tap whose hand arrived no later than this after the voice was last
/// seen speaking stopped the lamp: two looks at `voice/state`.
const STOPPED_SPEECH_US: u64 = 2 * NAME_LOOK_EVERY_US;

impl TapsIn {
    pub fn on(root: &Path) -> io::Result<Self> {
        let bus = Bus::new(root)?;
        Ok(Self {
            touches: bus.subscribe_typed(
                lamp_msgs::TouchEvent::TOPIC,
                SOURCE,
                lamp_msgs::TouchEvent::TYPE,
            )?,
            bus,
            looked_at_us: None,
            taken: None,
            speaking_seen_us: None,
        })
    }

    /// When the newest head tap since the last look was let go, by
    /// Controls' clock, which is this machine's monotonic clock. Never waits,
    /// and looks at the bus at most every 20 ms however often it is called.
    pub fn newest(&mut self, now_us: u64) -> Option<u64> {
        if self
            .looked_at_us
            .is_some_and(|at| now_us.saturating_sub(at) < NAME_LOOK_EVERY_US)
        {
            return None;
        }
        self.looked_at_us = Some(now_us);
        // Whether the lamp is speaking, each look: a tap whose hand arrived
        // while it was stopped it, and is no call (the reviewer on #1107;
        // the head's "hm?" holds back the same way, `head_cue::may_hum`).
        if self
            .bus
            .latest(VoiceState::TOPIC)
            .ok()
            .flatten()
            .filter(|message| !message.expired)
            .and_then(|message| message.parse::<VoiceState>().ok())
            .is_some_and(|voice| voice.state == Voice::Speaking)
        {
            self.speaking_seen_us = Some(now_us);
        }
        let mut newest = None;
        for _ in 0..8 {
            let Ok(Some(message)) = self.touches.recv(std::time::Duration::ZERO) else {
                break;
            };
            if let Some(released) = self.tap(&message, now_us) {
                newest = Some(newest.map_or(released, |before: u64| before.max(released)));
            }
        }
        newest
    }

    fn tap(&mut self, message: &lamp_bus::Message, now_us: u64) -> Option<u64> {
        let event = message.parse::<lamp_msgs::TouchEvent>().ok()?;
        let released = event.released_us?;
        let identity = (event.session_id.clone(), event.stamp.sequence);
        let fresh = !message.expired
            && message.header.source == "controls"
            && event.check().is_ok()
            && event.enabled
            && event.source == lamp_msgs::TouchSource::Head
            && event.gesture == lamp_msgs::TouchGesture::Tap
            && released <= now_us
            && now_us - released < TAP_FRESH_US
            && self.taken.as_ref() != Some(&identity);
        if fresh
            && self
                .speaking_seen_us
                .is_some_and(|seen| seen.saturating_add(STOPPED_SPEECH_US) >= event.began_us)
        {
            self.taken = Some(identity);
            eprintln!("lamp-voice provider: a head tap that stopped the lamp's speech is no call");
            return None;
        }
        fresh.then(|| {
            self.taken = Some(identity);
            released
        })
    }
}

/// How the room is, as the provider worker last published it
/// (`room/state`, [`RoomOut`]): unknown when nothing was in the last
/// [`ROOM_FRESH_MS`]. `room/state` carries no expiry, and a worker that has
/// stopped (when the second route stands in for Gemini's) leaves its last
/// room kept however old.
pub fn room_now(bus: &Bus) -> crate::room_told::Room {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis().try_into().unwrap_or(u64::MAX));
    bus.latest(RoomState::TOPIC)
        .ok()
        .flatten()
        .filter(|message| !message.expired)
        .and_then(|message| {
            let state = message.parse::<RoomState>().ok()?.state;
            Some(room_at(state, message.header.at_unix_ms, now_ms))
        })
        .unwrap_or(crate::room_told::Room::Unknown)
}

/// [`RoomOut`] says the room again every second; a value this old says
/// nothing of now.
pub const ROOM_FRESH_MS: u64 = 3_000;

/// A published room stamped `at_ms`, read at `now_ms` (both wall clock).
fn room_at(state: Room, at_ms: u64, now_ms: u64) -> crate::room_told::Room {
    if now_ms.saturating_sub(at_ms) > ROOM_FRESH_MS {
        return crate::room_told::Room::Unknown;
    }
    match state {
        Room::Unknown => crate::room_told::Room::Unknown,
        Room::Quiet => crate::room_told::Room::Quiet,
        Room::Busy => crate::room_told::Room::Busy,
    }
}

/// Whether the eyes see someone turned to the lamp now
/// (`humans/faces/tracked`, `docs/eyes.md`): true when a face in their
/// newest look is, false when that look saw nobody turned to it, nothing
/// when they have said nothing still fresh (not running, the camera closed,
/// the look older than its two seconds).
pub fn someone_turned_to_lamp(bus: &Bus) -> Option<bool> {
    #[cfg(test)]
    let mut diagnostic = crate::second_provider::worker::two_faces_diagnostic::LatestRead::begin(
        "someone_turned_to_lamp",
    );
    let message = bus.latest(lamp_msgs::Faces::TOPIC).ok()??;
    #[cfg(test)]
    diagnostic.message(&message);
    turned_in(&message)
}

fn turned_in(message: &lamp_bus::Message) -> Option<bool> {
    if message.expired {
        return None;
    }
    let faces = message.parse::<lamp_msgs::Faces>().ok()?;
    Some(faces.faces.iter().any(|face| face.facing == Some(true)))
}

/// Whether the eyes' newest fresh look has someone in view and nobody turned
/// to the lamp: a face seen turned away, none turned to it. A line said to
/// nobody then is people talking to each other (`LAMP_NAME_OR_FOLLOW_UP`,
/// `room_told::RoomTold::seen_talking`); with nobody in view it may be one
/// person whose face the eyes don't see.
pub fn someone_in_view_turned_away(bus: &Bus) -> bool {
    let Ok(Some(message)) = bus.latest(lamp_msgs::Faces::TOPIC) else {
        return false;
    };
    if message.expired {
        return false;
    }
    message.parse::<lamp_msgs::Faces>().is_ok_and(|faces| {
        faces.faces.iter().any(|face| face.facing == Some(false))
            && !faces.faces.iter().any(|face| face.facing == Some(true))
    })
}

/// How many faces the eyes see now (`humans/faces/tracked`), turned to the
/// lamp or not: in their newest look, nothing when they have said nothing
/// still fresh, as [`someone_turned_to_lamp`] reads them. Two or more is
/// people together at the lamp: the room is busy at once
/// (`LAMP_NAME_OR_FOLLOW_UP`, `room_told::RoomTold::two_people`), and a line
/// cut short is not joined with the next on no other sign
/// (`LAMP_JOIN_CUT_WORDS`). The second route reads it once a verdict for
/// both.
pub fn faces_in_view(bus: &Bus) -> Option<usize> {
    #[cfg(test)]
    let mut diagnostic =
        crate::second_provider::worker::two_faces_diagnostic::LatestRead::begin("faces_in_view");
    let message = bus.latest(lamp_msgs::Faces::TOPIC).ok()??;
    #[cfg(test)]
    diagnostic.message(&message);
    if message.expired {
        return None;
    }
    Some(message.parse::<lamp_msgs::Faces>().ok()?.faces.len())
}

/// The link to the speech service may be down this long before the voice
/// says anything is the matter. It is down for a second or two each time
/// the voice starts and each time the service renews its session, which
/// is the link working as it should.
pub const LINK_GRACE_US: u64 = 5_000_000;

/// The voice's own report (`diagnostics`): well while it can reach the
/// speech service, or has only just lost it. A voice mic waited for at the
/// latest capture start (#1178) is said in its message, so `lamp-health`
/// names it until the next capture start: alone while well, after the
/// link's warning otherwise.
pub fn health(
    link_up: bool,
    down_for_us: u64,
    refusal: Option<crate::provider_worker::Fault>,
    mic_at_start: Option<crate::wire::MicWaited>,
) -> lamp_msgs::Diagnostic {
    use crate::provider_worker::Fault;
    use lamp_msgs::{Diagnostic, Level};
    let well = |link: &str| match mic_at_start {
        Some(waited) => Diagnostic::new(Level::Ok, SOURCE, &waited.plain()).with("link", link),
        None => Diagnostic::ok(SOURCE).with("link", link),
    };
    // Refused, not unreachable (#2231): said at once and as it is, so a
    // person knows what the lamp needs rather than that it keeps trying.
    let refused = match refusal {
        _ if link_up => None,
        Some(Fault::Authentication) => Some((
            "the speech service refused this lamp's key: it needs a working key, from setup or the app's own keys",
            "key",
        )),
        Some(Fault::QuotaExceeded) => Some((
            "the speech service refuses this lamp for now (its allowance is used up); it asks again in a few minutes",
            "limited",
        )),
        Some(Fault::RateLimited) => Some((
            "the speech service is busy and turns this lamp away for now; it asks again shortly",
            "limited",
        )),
        _ => None,
    };
    if let Some((message, refused)) = refused {
        let message = match mic_at_start {
            Some(waited) => format!("{message}; {}", waited.plain()),
            None => message.to_owned(),
        };
        return Diagnostic::new(Level::Warn, SOURCE, &message)
            .with("link", "refused")
            .with("refused", refused);
    }
    if link_up {
        well("ready")
    } else if down_for_us < LINK_GRACE_US {
        well("connecting")
    } else {
        let message = "it cannot reach the speech service, and keeps trying";
        let message = match mic_at_start {
            Some(waited) => format!("{message}; {}", waited.plain()),
            None => message.to_owned(),
        };
        Diagnostic::new(Level::Warn, SOURCE, &message)
            .with("link", "down")
            .with("down_for_s", down_for_us / 1_000_000)
    }
}

/// The voice's report with, while `off`, that the light ring faulted past
/// its restarts and stays off this session (#1620): a warning, with the
/// fact `ring` = `off`. The voice goes on; what else is said comes after.
pub fn ring_off(report: lamp_msgs::Diagnostic, off: bool) -> lamp_msgs::Diagnostic {
    use lamp_msgs::{Diagnostic, Level};
    if !off {
        return report;
    }
    let said = "the light ring faulted and stays off until the voice starts again";
    let message = if report.message.is_empty() {
        said.to_owned()
    } else {
        format!("{said}; {}", report.message)
    };
    let level = match report.level {
        Level::Ok => Level::Warn,
        level => level,
    };
    let mut out = Diagnostic::new(level, SOURCE, &message);
    out.values = report.values;
    out.with("ring", "off")
}

/// The voice's report with, while `refused`, that a voice service (speech to text, the chat or speech) refused
/// this lamp's key turn after turn though the link is up (#2231,
/// `coordinator::key_refusals`): a warning, with `refused` = `key`. What
/// was wrong before is said after it.
pub fn chat_key_refused(report: lamp_msgs::Diagnostic, refused: bool) -> lamp_msgs::Diagnostic {
    use lamp_msgs::{Diagnostic, Level};
    if !refused {
        return report;
    }
    let said = "a voice service refused this lamp's key: it needs a working key, from setup or the app's own keys";
    let message = if report.message.is_empty() {
        said.to_owned()
    } else {
        format!("{said}; {}", report.message)
    };
    let level = match report.level {
        Level::Ok => Level::Warn,
        level => level,
    };
    let mut out = Diagnostic::new(level, SOURCE, &message);
    out.values = report.values;
    out.with("refused", "key")
}

/// The voice's report with, while `runs` is above 0, that so many runs in a
/// row failed before they heard anything (#1567): a warning, never that it
/// is muted, since it is not, and the count as the fact `could_not_start`.
/// What was wrong before is said after it.
pub fn could_not_start(report: lamp_msgs::Diagnostic, runs: u32) -> lamp_msgs::Diagnostic {
    use lamp_msgs::{Diagnostic, Level};
    if runs == 0 {
        return report;
    }
    let times = if runs == 1 { "time" } else { "times" };
    let said = format!(
        "the voice could not start ({runs} {times} since it last heard anything); trying again"
    );
    let message = if report.message.is_empty() {
        said
    } else {
        format!("{said}; {}", report.message)
    };
    let level = match report.level {
        Level::Ok => Level::Warn,
        level => level,
    };
    let mut out = Diagnostic::new(level, SOURCE, &message);
    out.values = report.values;
    out.with("could_not_start", runs)
}

/// Why the voice waits with no audio open (#1371): between sessions it
/// runs none, and nothing else says how it is, so `lamp-health` took a
/// lamp muted by its privacy switch for 2 h 25 min as a voice gone quiet
/// (lamp-06c1, 17 Oct).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Waiting {
    /// The privacy switch is closed.
    Switch,
    /// The switch has not been read for [`WAIT_UNREADABLE_AFTER_US`], so
    /// the microphones stay closed.
    Unreadable,
    /// Muted by the restart guard after a fault, not by choice (#960).
    RestartGuard,
    /// Asleep: a hand held on the head (#711).
    Asleep,
    /// Muted by the person.
    Muted,
    /// The switch reads open and nothing else holds it: a session is
    /// about to open.
    Opening,
    /// The switch is not read yet: the wait's first looks, before its
    /// worker's first reading.
    Reading,
}

/// A switch unread for this long while the voice waits is said to be
/// unreadable: the GPIO wait's own `UNAVAILABLE_WAIT`, and the Controls
/// route's retry. Before it, every wait's first looks would say a fault
/// that is not there (the reviewer and Jarvis on #1376).
pub const WAIT_UNREADABLE_AFTER_US: u64 = 1_000_000;

impl Waiting {
    /// Whether an owed reminder may pulse on the asleep face (#1545): only
    /// while the switch or the person's mute holds the microphones, not a
    /// hand on the head, whose face the asleep face is then, nor a fault
    /// (Thirteen on #1545).
    pub fn pulses(self) -> bool {
        matches!(self, Self::Switch | Self::Muted)
    }

    /// Whether the microphones are held closed: by the switch, a mute, a
    /// hand, the restart guard or a switch that cannot be read. Not while
    /// the switch is first read or has read open, which every start of the
    /// voice passes through, muted or not (the reviewer on #1551). Every
    /// case is named, so a new one must be placed here (Fifth on #1565).
    pub fn closed(self) -> bool {
        match self {
            Self::Switch | Self::Muted | Self::Asleep | Self::RestartGuard | Self::Unreadable => {
                true
            }
            Self::Reading | Self::Opening => false,
        }
    }

    /// From the switch as the wait last read it, whether it has gone
    /// unread for [`WAIT_UNREADABLE_AFTER_US`], whether the software mute
    /// lets the lamp hear, and why it is set. The worst reason first: a
    /// closed switch, or one open but not yet steady (a flap), is the
    /// microphones off whatever the mute says; then a mute nobody chose,
    /// then a switch that cannot be read, which a person's mute must not
    /// hide.
    pub fn of(
        slide: Option<crate::privacy::PrivacyState>,
        unreadable: bool,
        allowed: bool,
        asleep: bool,
        restored: bool,
    ) -> Self {
        use crate::privacy::PrivacyState;
        match slide {
            Some(PrivacyState::Muted | PrivacyState::Stabilizing) => Self::Switch,
            _ if restored => Self::RestartGuard,
            _ if unreadable => Self::Unreadable,
            _ if asleep => Self::Asleep,
            _ if !allowed => Self::Muted,
            Some(PrivacyState::Allowed) => Self::Opening,
            _ => Self::Reading,
        }
    }
}

/// The voice's report while it waits: well, saying the microphones are
/// off and why ([`lamp_msgs::MICROPHONES_OFF`]); a warning for a mute
/// nobody chose and a switch that cannot be read. With `owed`, a due
/// reminder waits for them to open, and the report says so ([`OWED`], and
/// `reminder` `owed`), at the same level (#1545).
pub fn waiting_health(waiting: Waiting, owed: bool) -> lamp_msgs::Diagnostic {
    use lamp_msgs::{Diagnostic, Level, MICROPHONES_OFF};
    let (level, why) = match waiting {
        Waiting::Switch => (Level::Ok, "(the privacy switch)"),
        Waiting::Muted => (Level::Ok, "(muted)"),
        Waiting::Asleep => (Level::Ok, "(asleep: a hand held on the head)"),
        Waiting::RestartGuard => (
            Level::Warn,
            "(muted by the restart guard after a fault, not by choice)",
        ),
        Waiting::Unreadable => (Level::Warn, "until the privacy switch can be read"),
        Waiting::Reading => (Level::Ok, "(reading the privacy switch)"),
        Waiting::Opening => {
            let opening = Diagnostic::ok(SOURCE).with("link", "connecting");
            return if owed {
                opening.with("reminder", "owed")
            } else {
                opening
            };
        }
    };
    let report = if owed {
        Diagnostic::new(level, SOURCE, &format!("{MICROPHONES_OFF} {why}; {OWED}"))
            .with("reminder", "owed")
    } else {
        Diagnostic::new(level, SOURCE, &format!("{MICROPHONES_OFF} {why}"))
    };
    report.with("link", "closed")
}

/// What the waiting voice's report adds while a due reminder waits.
pub const OWED: &str = "a reminder is owed, to be said when they open";

/// How each line sounded (`voice/tone`, `LAMP_VOICE_TONE`, [`crate::voice_tone`]).
pub struct ToneOut {
    tone: Publisher,
}

impl ToneOut {
    /// With `LAMP_BUS=on`, `LAMP_VOICE_TONE=on` and a bus that can be used;
    /// else nothing.
    pub fn from_environment() -> Option<Self> {
        if std::env::var(ENVIRONMENT).ok().as_deref() != Some("on")
            || !crate::voice_tone::on(
                std::env::var(crate::voice_tone::ENVIRONMENT)
                    .ok()
                    .as_deref(),
            )
        {
            return None;
        }
        Bus::from_environment().and_then(|bus| Self::on(&bus)).ok()
    }

    pub fn on(bus: &Bus) -> io::Result<Self> {
        Ok(Self {
            tone: bus.publisher_declared(
                lamp_msgs::VoiceTone::TOPIC,
                &lamp_msgs::VoiceTone::declaration(),
                SOURCE,
            )?,
        })
    }

    pub fn tell(&mut self, now_us: u64, tone: &lamp_msgs::VoiceTone) {
        let _ = self.tone.publish_json(now_us, tone);
    }
}

/// Memory's forgets (`memory/forgot`, the first builder at 10:5x on #2467),
/// for the voice's tone baselines: each person whose everything memory has
/// forgotten, asked by the app or by voice. Never waits, and looks at the
/// bus at most every half second however often it is asked.
pub struct ForgotIn {
    forgot: lamp_bus::Subscriber,
    looked_at_us: Option<u64>,
}

/// How often the bus is looked at for a forget: rare, and nothing waits on
/// it.
const FORGOT_LOOK_EVERY_US: u64 = 500_000;

impl ForgotIn {
    /// With `LAMP_BUS=on` and a bus that can be used; else nothing.
    pub fn from_environment() -> Option<Self> {
        if std::env::var(ENVIRONMENT).ok().as_deref() != Some("on") {
            return None;
        }
        Bus::from_environment().and_then(|bus| Self::on(&bus)).ok()
    }

    pub fn on(bus: &Bus) -> io::Result<Self> {
        Ok(Self {
            forgot: bus.subscribe_typed(
                lamp_msgs::MemoryForgot::TOPIC,
                SOURCE,
                lamp_msgs::MemoryForgot::TYPE,
            )?,
            looked_at_us: None,
        })
    }

    /// The forgets since the last look, at most eight: a person's id, or
    /// `None` for every person's. Only memory's, and only well formed.
    pub fn forgotten(&mut self, now_us: u64) -> Vec<Option<String>> {
        if self
            .looked_at_us
            .is_some_and(|at| now_us.saturating_sub(at) < FORGOT_LOOK_EVERY_US)
        {
            return Vec::new();
        }
        self.looked_at_us = Some(now_us);
        let mut forgotten = Vec::new();
        for _ in 0..8 {
            let Ok(Some(message)) = self.forgot.recv(std::time::Duration::ZERO) else {
                break;
            };
            if message.header.source != "memory" {
                continue;
            }
            if let Ok(forgot) = message.parse::<lamp_msgs::MemoryForgot>()
                && forgot.check().is_ok()
            {
                forgotten.push(forgot.person);
            }
        }
        forgotten
    }
}

/// The voice's health while it waits (#1371), as a running voice says it:
/// a little more often than once a second, and at once when why changes.
pub struct WaitHealth {
    health: Publisher,
    reported: Option<(u64, Waiting, bool)>,
}

impl WaitHealth {
    /// With `LAMP_BUS=on` and a bus that can be used; else nothing, as
    /// [`BusOut::from_environment`].
    pub fn from_environment() -> Option<Self> {
        if std::env::var(ENVIRONMENT).ok().as_deref() != Some("on") {
            return None;
        }
        Bus::from_environment().and_then(|bus| Self::on(&bus)).ok()
    }

    pub fn on(bus: &Bus) -> io::Result<Self> {
        Ok(Self {
            health: bus.publisher_declared(
                lamp_msgs::Diagnostic::TOPIC,
                &lamp_msgs::Diagnostic::declaration(),
                SOURCE,
            )?,
            reported: None,
        })
    }

    /// At a run's start, before any worker opens, while runs before it
    /// could not start (#1567): said at once, so a lamp whose voice keeps
    /// failing before it hears shows the count each time it tries, not only
    /// in a good run's first report (the reviewer on #1592).
    pub fn report_could_not_start(&mut self, now_us: u64, runs: u32) {
        let trying = lamp_msgs::Diagnostic::ok(SOURCE).with("link", "connecting");
        let _ = self
            .health
            .publish_json(now_us, &could_not_start(trying, runs));
        self.reported = None;
    }

    /// Call on every look of the wait, with whether a due reminder is owed
    /// ([`waiting_health`]): at once when either changes.
    pub fn report(&mut self, now_us: u64, waiting: Waiting, owed: bool) {
        if self.reported.is_some_and(|(at, said, was_owed)| {
            said == waiting && was_owed == owed && now_us.saturating_sub(at) < 950_000
        }) {
            return;
        }
        self.reported = Some((now_us, waiting, owed));
        let _ = self
            .health
            .publish_json(now_us, &waiting_health(waiting, owed));
    }
}

/// The link as `voice/link` says it: unavailable only once it has been
/// down for [`LINK_GRACE_US`], so a session renewing itself is not the
/// lamp going offline. A refusal (#2231) is said at once with its kind;
/// any other fault's kind is not known here, so it is said as a failed
/// connection.
pub fn link_state(
    link_up: bool,
    down_for_us: u64,
    refusal: Option<crate::provider_worker::Fault>,
) -> VoiceLink {
    use crate::provider_worker::Fault;
    if link_up {
        return VoiceLink::ready();
    }
    match refusal {
        Some(Fault::Authentication) => VoiceLink::unavailable(LinkFault::Authentication),
        Some(Fault::QuotaExceeded | Fault::RateLimited) => {
            VoiceLink::unavailable(LinkFault::Limited)
        }
        _ if down_for_us < LINK_GRACE_US => VoiceLink::ready(),
        _ => VoiceLink::unavailable(LinkFault::Transport),
    }
}

pub struct BusOut {
    onboarding: Option<Publisher>,
    last_onboarding: Option<(lamp_msgs::OnboardingPending, u64)>,
    state: Publisher,
    heard: Publisher,
    said: Publisher,
    health: Publisher,
    /// `voice/link`, with [`LINK_ENVIRONMENT`].
    link: Option<Publisher>,
    last: Option<(Voice, u64)>,
    /// When the voice last said how it is, and since when its link to the
    /// speech service has been down.
    reported_at_us: Option<u64>,
    down_since_us: Option<u64>,
    /// The turn whose input began last, if one has: what is heard is told
    /// as that turn's.
    heard_in: Option<u64>,
    /// The answer whose words have begun to be said, and one that opened
    /// with a function's name, whose words are none of the lamp's.
    said_in: Option<u64>,
    unsaid: Option<u64>,
    calls: crate::not_speech::CallsLeftOut,
    /// Runs in a row that failed before they heard anything (#1567), said
    /// in the health until this run hears.
    could_not_start: u32,
    /// The light ring faulted and stays off this session (#1620).
    ring_off: bool,
    chat_key_refused: bool,
}

impl BusOut {
    /// The publishers, if the environment says `on` and the bus can be
    /// used. A bus that cannot is said once and the voice goes on without.
    pub fn from_environment() -> Option<Self> {
        if std::env::var(ENVIRONMENT).ok().as_deref() != Some("on") {
            return None;
        }
        let link = std::env::var(LINK_ENVIRONMENT).ok().as_deref() == Some("on");
        match Bus::from_environment().and_then(|bus| {
            Self::on(&bus)?.with_link(&bus, link)?.with_onboarding(
                &bus,
                std::env::var(crate::onboarding::ENVIRONMENT)
                    .ok()
                    .as_deref()
                    == Some("on"),
            )
        }) {
            Ok(out) => Some(out),
            Err(error) => {
                eprintln!("lamp-voice: nothing is published on the bus: {error}");
                None
            }
        }
    }

    pub fn on(bus: &Bus) -> io::Result<Self> {
        let [heard, said] = Speech::TOPICS;
        Ok(Self {
            onboarding: None,
            last_onboarding: None,
            state: bus.publisher_declared(VoiceState::TOPIC, &VoiceState::declaration(), SOURCE)?,
            heard: bus.publisher_declared(heard, &Speech::declaration(), SOURCE)?,
            said: bus.publisher_declared(said, &Speech::declaration(), SOURCE)?,
            health: bus.publisher_declared(
                lamp_msgs::Diagnostic::TOPIC,
                &lamp_msgs::Diagnostic::declaration(),
                SOURCE,
            )?,
            link: None,
            last: None,
            reported_at_us: None,
            down_since_us: None,
            heard_in: None,
            said_in: None,
            unsaid: None,
            calls: crate::not_speech::CallsLeftOut::new(),
            could_not_start: 0,
            ring_off: false,
            chat_key_refused: false,
        })
    }

    /// Only enabled onboarding publishes invitation identities. This carries no
    /// sensor data and never grants microphone or camera authority.
    pub fn with_onboarding(mut self, bus: &Bus, enabled: bool) -> io::Result<Self> {
        if enabled {
            self.onboarding = Some(bus.publisher_declared(
                lamp_msgs::OnboardingPending::TOPIC,
                &lamp_msgs::OnboardingPending::declaration(),
                SOURCE,
            )?);
        }
        Ok(self)
    }

    pub fn onboarding_pending(&mut self, value: &lamp_msgs::OnboardingPending, now_us: u64) {
        let Some(out) = self.onboarding.as_mut() else {
            return;
        };
        if value.check().is_err()
            || self
                .last_onboarding
                .as_ref()
                .is_some_and(|(old, at)| old == value && now_us.saturating_sub(*at) < 500_000)
        {
            return;
        }
        // Failed publication is retried on the next pass, never counted as an
        // invitation delivered to the person or consumed by the core skill.
        if out.publish_json(now_us, value).is_ok() {
            self.last_onboarding = Some((value.clone(), now_us));
        }
    }

    /// The service began to take in the person's words for turn `request`.
    pub fn input_started(&mut self, request: u64) {
        self.heard_in = Some(request);
    }

    /// Publishes `voice/link` as well, when `on`.
    pub fn with_link(mut self, bus: &Bus, on: bool) -> io::Result<Self> {
        if on {
            self.link = Some(bus.publisher_declared(
                VoiceLink::TOPIC,
                &VoiceLink::declaration(),
                SOURCE,
            )?);
        }
        Ok(self)
    }

    /// The phase of the turn that owns the lamp, or none between turns.
    /// Call every tick: it publishes on a change and once a second.
    pub fn state(&mut self, phase: Option<RingPhase>, now_us: u64) {
        let state = match phase {
            None => Voice::Resting,
            Some(RingPhase::Listening) => Voice::Listening,
            Some(RingPhase::Waiting) => Voice::Thinking,
            Some(RingPhase::Speaking) => Voice::Speaking,
        };
        if self
            .last
            .is_some_and(|(told, at)| told == state && now_us.saturating_sub(at) < REPEAT_US)
        {
            return;
        }
        self.last = Some((state, now_us));
        let _ = self.state.publish_json(now_us, &VoiceState { state });
        // The two streams are silent between things said, for longer than
        // the bus's lease: without this their subscribers are told, each
        // time, that the voice has gone. On lamp-4ace on 2026-10-14 they
        // were, before and after every sentence.
        let _ = self.heard.alive();
        let _ = self.said.alive();
    }

    /// The report said now, whatever was said last (#2231, the reviewer on
    /// #2459): a run that ends on a refusal says why before it leaves, even
    /// within the 950 ms after its last report.
    pub fn report_now(
        &mut self,
        link_up: bool,
        refusal: Option<crate::provider_worker::Fault>,
        now_us: u64,
        mic_at_start: Option<crate::wire::MicWaited>,
    ) {
        self.reported_at_us = None;
        self.report(link_up, refusal, now_us, mic_at_start);
    }

    /// Whether the voice can reach the speech service now. Call every
    /// tick: how the voice is, is said a little more often than once a
    /// second, for the lamp's health.
    pub fn report(
        &mut self,
        link_up: bool,
        refusal: Option<crate::provider_worker::Fault>,
        now_us: u64,
        mic_at_start: Option<crate::wire::MicWaited>,
    ) {
        if link_up {
            self.down_since_us = None;
        } else {
            self.down_since_us.get_or_insert(now_us);
        }
        if self
            .reported_at_us
            .is_some_and(|at| now_us.saturating_sub(at) < 950_000)
        {
            return;
        }
        self.reported_at_us = Some(now_us);
        let down_for = self
            .down_since_us
            .map_or(0, |since| now_us.saturating_sub(since));
        let report = ring_off(
            could_not_start(
                chat_key_refused(
                    health(link_up, down_for, refusal, mic_at_start),
                    self.chat_key_refused,
                ),
                self.could_not_start,
            ),
            self.ring_off,
        );
        let _ = self.health.publish_json(now_us, &report);
        if let Some(link) = &mut self.link {
            let _ = link.publish_json(now_us, &link_state(link_up, down_for, refusal));
        }
    }

    /// Whether the light ring stays off this session (#1620): said with
    /// each report while it does.
    pub fn set_ring_off(&mut self, off: bool) {
        self.ring_off = off;
    }

    /// Whether a voice service refuses this lamp's key turn after turn
    /// (#2231): said with each report while it does.
    pub fn set_chat_key_refused(&mut self, refused: bool) {
        self.chat_key_refused = refused;
    }

    /// How many runs in a row failed before they heard anything, as the
    /// hand controls keep it (#1567): said with each report while above 0.
    pub fn set_could_not_start(&mut self, runs: u32) {
        self.could_not_start = runs;
    }

    /// A piece of the provider's text of what a person said, with the turn
    /// it was heard in: the one whose input began last
    /// ([`Self::input_started`]), if one has. The service does not say which
    /// turn its text of what was heard is of; a piece it writes only after
    /// the next turn's input began is taken as that next turn's. The
    /// Policy's decision about the words carries the number, and the lamp's
    /// actions are paired with it (`crate::policy_holds`).
    pub fn heard(&mut self, text: &str, finished: bool, now_us: u64) {
        let _ = self
            .heard
            .publish_json(now_us, &speech(self.heard_in, text, finished));
    }

    /// A piece of the provider's text of the lamp's answer to `request`.
    /// A function's name written where it should have been called is never
    /// said: an answer that opens with one has none of its words published,
    /// as the thread keeps none of them, and a piece that is a whole call
    /// (`not_for_me{}`) is left out wherever it comes (lamp-4ace,
    /// 2026-10-16 17:12).
    pub fn said(&mut self, request: u64, text: &str, finished: bool, now_us: u64) {
        if self.unsaid == Some(request) {
            return;
        }
        let opening = self.said_in != Some(request);
        if opening && crate::not_speech::names_a_function(text) {
            self.unsaid = Some(request);
            return;
        }
        if !self.calls.keeps(request, text) {
            return;
        }
        self.said_in = Some(request);
        let _ = self
            .said
            .publish_json(now_us, &speech(Some(request), text, finished));
    }
}

fn speech(request: Option<u64>, text: &str, finished: bool) -> Speech {
    Speech {
        text: text.to_owned(),
        finished,
        locale: None,
        request,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// A root short enough for a socket's path, which the system's own
    /// temporary directory on a Mac is not.
    fn bus(name: &str) -> (Bus, std::path::PathBuf) {
        let root = std::path::PathBuf::from(format!("/tmp/lvb-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        (Bus::new(&root).unwrap(), root)
    }

    /// Memory's forgets reach the voice's tone baselines: a person's id, or
    /// none for every person; only memory's and only well formed; and the
    /// bus is looked at at most every half second.
    #[test]
    fn memorys_forgets_are_read_from_memory_only_at_most_twice_a_second() {
        let (bus, root) = bus("forgot");
        let mut forgot = ForgotIn::on(&bus).unwrap();
        let mut memory = bus
            .publisher_declared(
                lamp_msgs::MemoryForgot::TOPIC,
                &lamp_msgs::MemoryForgot::declaration(),
                "memory",
            )
            .unwrap();
        let mut other = bus
            .publisher_declared(
                lamp_msgs::MemoryForgot::TOPIC,
                &lamp_msgs::MemoryForgot::declaration(),
                "eyes",
            )
            .unwrap();
        let person = lamp_msgs::known::person_id("Jim");
        let told = |person: Option<&str>| lamp_msgs::MemoryForgot {
            person: person.map(str::to_owned),
        };
        memory.publish_json(1, &told(Some(&person))).unwrap();
        other.publish_json(2, &told(Some(&person))).unwrap();
        memory.publish_json(3, &told(Some("NOT AN ID"))).unwrap();
        memory.publish_json(4, &told(None)).unwrap();
        assert_eq!(forgot.forgotten(1_000_000), [Some(person.clone()), None]);
        memory.publish_json(5, &told(Some(&person))).unwrap();
        assert!(forgot.forgotten(1_400_000).is_empty());
        assert_eq!(forgot.forgotten(1_500_000), [Some(person)]);
        let _ = std::fs::remove_dir_all(root);
    }
    const WAIT: Duration = Duration::from_millis(500);

    /// #2231: a refusal is said at once and as it is, the key's and the
    /// quota's apart, on `voice/link` too, and not once the link is up.
    #[test]
    fn a_refused_key_or_quota_is_said_as_it_is_and_at_once() {
        use crate::provider_worker::Fault;
        let key = health(false, 0, Some(Fault::Authentication), None);
        assert_eq!(key.level, lamp_msgs::Level::Warn);
        assert!(
            key.message.contains("refused this lamp's key"),
            "{}",
            key.message
        );
        assert!(!key.message.contains("cannot reach"), "{}", key.message);
        assert_eq!(
            link_state(false, 0, Some(Fault::Authentication)),
            VoiceLink::unavailable(LinkFault::Authentication)
        );
        let quota = health(false, 0, Some(Fault::QuotaExceeded), None);
        assert!(
            quota.message.contains("allowance is used up"),
            "{}",
            quota.message
        );
        // A 429 is said as busy, not as a spent quota (the reviewer-lead on #2459).
        let busy = health(false, 0, Some(Fault::RateLimited), None);
        assert!(busy.message.contains("busy"), "{}", busy.message);
        assert!(!busy.message.contains("allowance"), "{}", busy.message);
        assert_eq!(
            link_state(false, 0, Some(Fault::RateLimited)),
            VoiceLink::unavailable(LinkFault::Limited)
        );
        // Any other fault is the link down, after its grace, as before.
        let other = health(false, LINK_GRACE_US, Some(Fault::Transport), None);
        assert!(other.message.contains("cannot reach"), "{}", other.message);
        assert_eq!(
            link_state(false, 0, Some(Fault::Transport)),
            VoiceLink::ready()
        );
        // A link up again is well, whatever was refused before.
        assert_eq!(
            health(true, 0, Some(Fault::Authentication), None).level,
            lamp_msgs::Level::Ok
        );
    }

    /// #2231 (the lead on #1711): a key a voice service refuses turn after
    /// turn is said while the link is up, beside what else is said.
    #[test]
    fn a_chat_key_refused_turn_after_turn_is_said_while_the_link_is_up() {
        let well = health(true, 0, None, None);
        assert_eq!(chat_key_refused(well.clone(), false), well);
        let refused = chat_key_refused(well, true);
        assert_eq!(refused.level, lamp_msgs::Level::Warn);
        assert!(
            refused
                .message
                .starts_with("a voice service refused this lamp's key"),
            "{}",
            refused.message
        );
        let has = |key: &str, value: &str| {
            refused
                .values
                .iter()
                .any(|fact| fact.key == key && fact.value == value)
        };
        assert!(has("refused", "key") && has("link", "ready"));
        // Said first, with what else was the matter after it.
        let down = chat_key_refused(health(false, LINK_GRACE_US, None, None), true);
        assert!(
            down.message
                .starts_with("a voice service refused this lamp's key")
                && down
                    .message
                    .ends_with("cannot reach the speech service, and keeps trying"),
            "{}",
            down.message
        );
    }

    /// A kept room older than `ROOM_FRESH_MS` is no room: the worker that
    /// published it may have stopped (Third on #1032).
    #[test]
    fn a_kept_room_says_nothing_once_it_is_older_than_three_seconds() {
        use crate::room_told::Room as Told;
        let now = 100_000;
        assert_eq!(room_at(Room::Busy, now - 1_000, now), Told::Busy);
        assert_eq!(room_at(Room::Quiet, now - ROOM_FRESH_MS, now), Told::Quiet);
        assert_eq!(room_at(Room::Busy, now - 10_000, now), Told::Unknown);
        assert_eq!(room_at(Room::Unknown, now, now), Told::Unknown);
        // A clock stepped back reads as fresh, not as never.
        assert_eq!(room_at(Room::Busy, now + 500, now), Told::Busy);
    }

    /// #1371: the voice waiting on a closed privacy switch says so as a
    /// well voice with its microphones off; why it waits is read switch
    /// first.
    #[test]
    fn a_waiting_voice_says_why_its_microphones_are_off() {
        use crate::privacy::PrivacyState::*;
        // (slide, unreadable, allowed, asleep, restored)
        let of = Waiting::of;
        assert_eq!(of(Some(Muted), false, true, false, false), Waiting::Switch);
        assert_eq!(of(Some(Muted), true, false, true, true), Waiting::Switch);
        // A flap: open, not yet steady, is still the switch.
        assert_eq!(
            of(Some(Stabilizing), false, true, false, false),
            Waiting::Switch
        );
        assert_eq!(
            of(Some(Allowed), false, false, false, true),
            Waiting::RestartGuard
        );
        assert_eq!(
            of(Some(Allowed), false, false, true, false),
            Waiting::Asleep
        );
        assert_eq!(of(None, false, false, false, false), Waiting::Muted);
        // Unread past the grace: said, even beside a person's mute.
        assert_eq!(
            of(Some(Unavailable), true, true, false, false),
            Waiting::Unreadable
        );
        assert_eq!(
            of(Some(Unavailable), true, false, true, false),
            Waiting::Unreadable
        );
        // Not read yet: no fault.
        assert_eq!(
            of(Some(Unavailable), false, true, false, false),
            Waiting::Reading
        );
        assert_eq!(of(None, false, true, false, false), Waiting::Reading);
        assert_eq!(
            of(Some(Allowed), false, true, false, false),
            Waiting::Opening
        );
        assert_eq!(
            waiting_health(Waiting::Reading, false).level,
            lamp_msgs::Level::Ok
        );
        let switch = waiting_health(Waiting::Switch, false);
        assert_eq!(switch.level, lamp_msgs::Level::Ok);
        assert_eq!(
            switch.message,
            "the microphones are off (the privacy switch)"
        );
        assert!(switch.message.starts_with(lamp_msgs::MICROPHONES_OFF));
        assert_eq!(
            waiting_health(Waiting::RestartGuard, false).level,
            lamp_msgs::Level::Warn
        );
        assert_eq!(
            waiting_health(Waiting::Unreadable, false).level,
            lamp_msgs::Level::Warn
        );
        assert!(waiting_health(Waiting::Opening, false).message.is_empty());
        // #1545: a due reminder waiting is said beside why, at the same
        // level, and marked; the opening report carries the mark only.
        let owed = waiting_health(Waiting::Muted, true);
        assert_eq!(owed.level, lamp_msgs::Level::Ok);
        assert_eq!(
            owed.message,
            "the microphones are off (muted); a reminder is owed, to be said when they open"
        );
        let unread = waiting_health(Waiting::Unreadable, true);
        assert_eq!(unread.level, lamp_msgs::Level::Warn);
        assert!(unread.message.ends_with(OWED));
        let opening = waiting_health(Waiting::Opening, true);
        assert!(opening.message.is_empty());
        let marked = |report: &lamp_msgs::Diagnostic| {
            report
                .values
                .iter()
                .any(|fact| fact.key == "reminder" && fact.value == "owed")
        };
        assert!(marked(&owed) && marked(&unread) && marked(&opening));
        assert!(!marked(&waiting_health(Waiting::Muted, false)));
        // Owed is counted only while the microphones are held closed (the
        // reviewer on #1551).
        for closed in [
            Waiting::Switch,
            Waiting::Muted,
            Waiting::Asleep,
            Waiting::RestartGuard,
            Waiting::Unreadable,
        ] {
            assert!(closed.closed(), "{closed:?}");
        }
        assert!(!Waiting::Reading.closed() && !Waiting::Opening.closed());
    }

    /// #1567 (the reviewer on #1592): the count is said on the bus as each
    /// run starts, as lamp-health reads it.
    #[test]
    fn a_run_starting_after_runs_that_could_not_start_says_the_count() {
        let (bus, root) = bus("start");
        let mut hears = bus
            .subscribe_typed(
                lamp_msgs::Diagnostic::TOPIC,
                "test",
                lamp_msgs::Diagnostic::TYPE,
            )
            .unwrap();
        let mut health = WaitHealth::on(&bus).unwrap();
        health.report_could_not_start(5, 2);
        let said = hears
            .recv(WAIT)
            .unwrap()
            .unwrap()
            .parse::<lamp_msgs::Diagnostic>()
            .unwrap();
        assert_eq!(said.level, lamp_msgs::Level::Warn);
        assert!(
            said.message
                .starts_with("the voice could not start (2 times")
        );
        assert!(
            said.values
                .iter()
                .any(|fact| fact.key == "could_not_start" && fact.value == "2")
        );
        assert!(
            said.values
                .iter()
                .any(|fact| fact.key == "link" && fact.value == "connecting")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// #1567: runs that could not start are said as a warning beside what
    /// else is wrong, never as a mute, and not at all once one has heard.
    #[test]
    fn a_ring_off_for_good_is_a_warning_with_its_fact_and_the_rest_after() {
        use lamp_msgs::Level;
        let well = health(true, 0, None, None);
        assert_eq!(ring_off(well.clone(), false), well);
        let off = ring_off(well, true);
        assert_eq!(off.level, Level::Warn);
        assert_eq!(
            off.message,
            "the light ring faulted and stays off until the voice starts again"
        );
        let fact = |report: &lamp_msgs::Diagnostic, key: &str| {
            report
                .values
                .iter()
                .find(|fact| fact.key == key)
                .map(|fact| fact.value.clone())
        };
        assert_eq!(fact(&off, "ring").as_deref(), Some("off"));
        assert_eq!(fact(&off, "link").as_deref(), Some("ready"));
        let down = ring_off(
            could_not_start(health(false, LINK_GRACE_US, None, None), 1),
            true,
        );
        assert_eq!(down.level, Level::Warn);
        assert!(down.message.starts_with(
            "the light ring faulted and stays off until the voice starts again; the voice could not start"
        ));
    }

    #[test]
    fn runs_that_could_not_start_are_a_warning_never_a_mute() {
        use lamp_msgs::Level;
        let well = health(true, 0, None, None);
        assert_eq!(could_not_start(well.clone(), 0), well);
        let once = could_not_start(well.clone(), 1);
        assert_eq!(once.level, Level::Warn);
        assert_eq!(
            once.message,
            "the voice could not start (1 time since it last heard anything); trying again"
        );
        assert!(
            once.values
                .iter()
                .any(|fact| fact.key == "link" && fact.value == "ready")
        );
        assert!(
            once.values
                .iter()
                .any(|fact| fact.key == "could_not_start" && fact.value == "1")
        );
        assert!(!once.message.contains("muted"));
        let down = could_not_start(health(false, LINK_GRACE_US, None, None), 3);
        assert_eq!(down.level, Level::Warn);
        assert!(
            down.message
                .starts_with("the voice could not start (3 times")
        );
        assert!(
            down.message
                .ends_with("it cannot reach the speech service, and keeps trying")
        );
    }

    /// #1545: a reminder coming due, or said, in the wait is told at once,
    /// not a second later.
    #[test]
    fn a_reminder_owed_in_the_wait_is_said_at_once() {
        let (bus, root) = bus("owed");
        let mut health = WaitHealth::on(&bus).unwrap();
        health.report(0, Waiting::Switch, false);
        let first = health.reported;
        health.report(2_000, Waiting::Switch, true);
        assert_ne!(health.reported, first);
        assert_eq!(health.reported, Some((2_000, Waiting::Switch, true)));
        health.report(4_000, Waiting::Switch, true);
        assert_eq!(health.reported, Some((2_000, Waiting::Switch, true)));
        health.report(6_000, Waiting::Switch, false);
        assert_eq!(health.reported, Some((6_000, Waiting::Switch, false)));
        let _ = std::fs::remove_dir_all(root);
    }

    /// #1371: an hour in the privacy wait, looked at every 2 ms, is said
    /// on `diagnostics` at least once a second throughout, so `lamp-health`
    /// (stale after 5 s) never takes it for a voice gone quiet; and a new
    /// reason is said at once.
    #[test]
    fn an_hour_in_the_privacy_wait_never_goes_quiet() {
        let (bus, root) = bus("wait");
        let mut health = WaitHealth::on(&bus).unwrap();
        let mut said = Vec::new();
        let hour_us = 3_600_000_000;
        let mut now = 0;
        while now <= hour_us {
            let before = health.reported;
            health.report(now, Waiting::Switch, false);
            if health.reported != before {
                said.push(now);
            }
            now += 2_000;
        }
        assert!(said.windows(2).all(|pair| pair[1] - pair[0] <= 1_000_000));
        assert!(hour_us - said.last().unwrap() <= 1_000_000);
        assert!(said.len() > 3_600);
        // On the bus, as `lamp-health` reads it: the switch once, then, the
        // switch opened, an empty report at once, not a second later.
        let mut hears = bus
            .subscribe_typed(
                lamp_msgs::Diagnostic::TOPIC,
                "test",
                lamp_msgs::Diagnostic::TYPE,
            )
            .unwrap();
        let mut health = WaitHealth::on(&bus).unwrap();
        health.report(0, Waiting::Switch, false);
        health.report(2_000, Waiting::Switch, false);
        health.report(4_000, Waiting::Opening, false);
        let mut heard = Vec::new();
        while let Some(message) = hears.recv(Duration::from_millis(50)).unwrap() {
            heard.push(message.parse::<lamp_msgs::Diagnostic>().unwrap().message);
        }
        assert_eq!(heard, ["the microphones are off (the privacy switch)", ""]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_link_is_unavailable_only_after_its_grace_and_published_only_when_asked() {
        assert_eq!(link_state(true, 0, None), VoiceLink::ready());
        assert_eq!(
            link_state(false, LINK_GRACE_US - 1, None),
            VoiceLink::ready()
        );
        let down = link_state(false, LINK_GRACE_US, None);
        assert_eq!(down, VoiceLink::unavailable(LinkFault::Transport));
        assert!(down.check().is_ok());
        let (bus, root) = bus("link");
        let mut off = BusOut::on(&bus).unwrap().with_link(&bus, false).unwrap();
        off.report(false, None, 0, None);
        off.report(false, None, LINK_GRACE_US + 1_000_000, None);
        assert!(bus.latest(VoiceLink::TOPIC).unwrap().is_none());
        let mut on = BusOut::on(&bus).unwrap().with_link(&bus, true).unwrap();
        on.report(false, None, 0, None);
        on.report(false, None, LINK_GRACE_US + 1_000_000, None);
        let latest = bus.latest(VoiceLink::TOPIC).unwrap().unwrap();
        assert_eq!(
            latest.parse::<VoiceLink>().unwrap(),
            VoiceLink::unavailable(LinkFault::Transport)
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn onboarding_is_opt_in_and_retries_publish_only_valid_bounded_identity() {
        use lamp_msgs::{OnboardingInvitation, OnboardingPending, OnboardingStep};
        let (bus, root) = bus("onboard");
        let mut pending = OnboardingPending {
            invitations: vec![OnboardingInvitation {
                invitation: 7,
                generation: 2,
                attempt: 1,
                step: OnboardingStep::Privacy,
                ready: true,
            }],
        };
        let mut off = BusOut::on(&bus)
            .unwrap()
            .with_onboarding(&bus, false)
            .unwrap();
        off.onboarding_pending(&pending, 10);
        assert!(bus.latest(OnboardingPending::TOPIC).unwrap().is_none());
        let mut on = BusOut::on(&bus)
            .unwrap()
            .with_onboarding(&bus, true)
            .unwrap();
        on.onboarding_pending(&pending, 10);
        let first = bus.latest(OnboardingPending::TOPIC).unwrap().unwrap();
        assert_eq!(first.parse::<OnboardingPending>().unwrap(), pending);
        on.onboarding_pending(&pending, 11);
        assert_eq!(
            bus.latest(OnboardingPending::TOPIC)
                .unwrap()
                .unwrap()
                .header
                .seq,
            first.header.seq
        );
        pending.invitations[0].ready = false;
        on.onboarding_pending(&pending, 12);
        let stopped = bus.latest(OnboardingPending::TOPIC).unwrap().unwrap();
        assert!(stopped.header.seq > first.header.seq);
        pending.invitations[0].invitation = 0;
        on.onboarding_pending(&pending, 13);
        assert_eq!(
            bus.latest(OnboardingPending::TOPIC)
                .unwrap()
                .unwrap()
                .header
                .seq,
            stopped.header.seq
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_state_is_published_when_it_changes_and_once_a_second() {
        let (bus, root) = bus("state");
        let mut out = BusOut::on(&bus).unwrap();
        let mut watch = bus
            .subscribe_typed(VoiceState::TOPIC, "watch", VoiceState::TYPE)
            .unwrap();
        // Subscription has bound and named the socket; first publication
        // scans for it synchronously, before the later rescan interval.
        let mut said = Vec::new();
        for (phase, at_us) in [
            (None, 0),
            (None, 500_000),
            (Some(RingPhase::Listening), 600_000),
            (Some(RingPhase::Listening), 900_000),
            (Some(RingPhase::Waiting), 1_000_000),
            (Some(RingPhase::Speaking), 1_100_000),
            (Some(RingPhase::Speaking), 2_099_999),
            (Some(RingPhase::Speaking), 2_100_000),
        ] {
            out.state(phase, at_us);
            while let Some(message) = watch.recv(Duration::from_millis(30)).unwrap() {
                said.push((message.parse::<VoiceState>().unwrap().state, at_us));
            }
        }
        assert_eq!(
            said,
            [
                (Voice::Resting, 0),
                (Voice::Listening, 600_000),
                (Voice::Thinking, 1_000_000),
                (Voice::Speaking, 1_100_000),
                (Voice::Speaking, 2_100_000),
            ]
        );
        // Whoever starts late reads how things stand.
        let latest = bus.latest(VoiceState::TOPIC).unwrap().unwrap();
        assert_eq!(latest.parse::<VoiceState>().unwrap().state, Voice::Speaking);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn someone_is_turned_to_the_lamp_only_when_the_eyes_newest_fresh_look_says_so() {
        use lamp_msgs::{Face, Faces};
        let (bus, root) = bus("faces");
        // No eyes: nothing known.
        assert_eq!(someone_turned_to_lamp(&bus), None);
        let mut eyes = bus
            .publisher_declared(Faces::TOPIC, &Faces::declaration(), "eyes")
            .unwrap();
        eyes.expires_after(Some(Duration::from_millis(300)));
        let face = |facing: Option<bool>| Face {
            speaking: None,
            id: "f1".into(),
            xmin: 0.4,
            ymin: 0.3,
            xmax: 0.6,
            ymax: 0.6,
            confidence: 1.0,
            yaw: facing.map(|_| 0.1),
            facing,
        };
        let say = |eyes: &mut lamp_bus::Publisher, faces: Vec<Face>| {
            eyes.publish_json(lamp_ipc::monotonic_us(), &Faces { faces })
                .unwrap();
        };
        say(&mut eyes, vec![]);
        assert_eq!(someone_turned_to_lamp(&bus), Some(false));
        say(&mut eyes, vec![face(Some(false)), face(None)]);
        assert_eq!(someone_turned_to_lamp(&bus), Some(false));
        say(&mut eyes, vec![face(None), face(Some(true))]);
        assert_eq!(someone_turned_to_lamp(&bus), Some(true));
        // A look older than its life says nothing.
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(someone_turned_to_lamp(&bus), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// `LAMP_NAME_OR_FOLLOW_UP` and `LAMP_JOIN_CUT_WORDS`: the faces in the
    /// eyes' newest fresh look, turned to the lamp or not, are counted; none
    /// fresh, none known.
    #[test]
    fn the_faces_in_view_are_counted_only_from_the_eyes_newest_fresh_look() {
        use lamp_msgs::{Face, Faces};
        let (bus, root) = bus("faces-counted");
        assert_eq!(faces_in_view(&bus), None);
        let mut eyes = bus
            .publisher_declared(Faces::TOPIC, &Faces::declaration(), "eyes")
            .unwrap();
        eyes.expires_after(Some(Duration::from_millis(300)));
        let face = |id: &str, facing: Option<bool>| Face {
            speaking: None,
            id: id.into(),
            xmin: 0.4,
            ymin: 0.3,
            xmax: 0.6,
            ymax: 0.6,
            confidence: 1.0,
            yaw: facing.map(|_| 0.1),
            facing,
        };
        let say = |eyes: &mut lamp_bus::Publisher, faces: Vec<Face>| {
            eyes.publish_json(lamp_ipc::monotonic_us(), &Faces { faces })
                .unwrap();
        };
        say(&mut eyes, vec![]);
        assert_eq!(faces_in_view(&bus), Some(0));
        say(&mut eyes, vec![face("f1", Some(true))]);
        assert_eq!(faces_in_view(&bus), Some(1));
        say(
            &mut eyes,
            vec![face("f1", Some(true)), face("f2", Some(false))],
        );
        assert_eq!(faces_in_view(&bus), Some(2));
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(faces_in_view(&bus), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_voice_is_well_while_it_can_reach_the_service_or_has_only_just_lost_it() {
        use lamp_msgs::{Diagnostic, Level};
        let facts = |report: &Diagnostic| -> Vec<(String, String)> {
            report
                .values
                .iter()
                .map(|fact| (fact.key.clone(), fact.value.clone()))
                .collect()
        };
        let up = health(true, 0, None, None);
        assert_eq!(
            (up.level, up.name.as_str(), up.message.as_str()),
            (Level::Ok, "voice", "")
        );
        assert_eq!(facts(&up), [("link".to_string(), "ready".to_string())]);
        // Down for a moment, as at every start and every renewal: still well.
        let renewing = health(false, LINK_GRACE_US - 1, None, None);
        assert_eq!(renewing.level, Level::Ok);
        assert_eq!(
            facts(&renewing),
            [("link".to_string(), "connecting".to_string())]
        );
        // Down for longer: said, with for how long.
        let down = health(false, 42_500_000, None, None);
        assert_eq!(
            (down.level, down.message.as_str()),
            (
                Level::Warn,
                "it cannot reach the speech service, and keeps trying"
            )
        );
        assert_eq!(
            facts(&down),
            [
                ("link".to_string(), "down".to_string()),
                ("down_for_s".to_string(), "42".to_string())
            ]
        );
        // A voice mic waited for at start (#1178): the message while well,
        // and after the warning's, so `lamp-health` names it either way.
        let waited = Some(crate::wire::MicWaited {
            gone: true,
            waited_ms: 4_300,
            opened_on_try: 18,
        });
        let noted = health(true, 0, None, waited);
        assert_eq!(
            (noted.level, noted.message.as_str()),
            (
                Level::Ok,
                "the voice mic was off USB for 4.3 s at start; opened on try 18"
            )
        );
        let down = health(false, 42_500_000, None, waited);
        assert_eq!(
            (down.level, down.message.as_str()),
            (
                Level::Warn,
                "it cannot reach the speech service, and keeps trying; the voice mic was off USB for 4.3 s at start; opened on try 18"
            )
        );
        assert_eq!(facts(&down).len(), 2, "link and down_for_s, as before");
        // On the bus: a little more often than once a second, and the
        // time the link has been down is counted from when it went.
        let (bus, root) = bus("health");
        let mut hears = bus
            .subscribe_typed(Diagnostic::TOPIC, "test", Diagnostic::TYPE)
            .unwrap();
        let mut out = BusOut::on(&bus).unwrap();
        // Nine seconds of ticks, 2 ms apart: up for one, then down.
        for tick in 0..4_500u64 {
            out.report(tick < 500, None, tick * 2_000, None);
        }
        let mut said = Vec::new();
        while let Some(message) = hears.recv(Duration::from_millis(50)).unwrap() {
            said.push(message.parse::<Diagnostic>().unwrap());
        }
        assert_eq!(said.len(), 10, "{said:?}");
        assert_eq!(said[0].level, Level::Ok);
        assert_eq!(facts(&said[1])[0].1, "ready");
        assert_eq!(facts(&said[2])[0].1, "connecting");
        assert_eq!(said[5].level, Level::Ok);
        assert_eq!(said[7].level, Level::Warn);
        assert_eq!(
            facts(&said[9])[1],
            ("down_for_s".to_string(), "7".to_string())
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_hearing_of_the_name_is_taken_from_the_bus_dated_from_when_it_began() {
        use lamp_msgs::NameHeard;
        let (bus, root) = bus("name");
        let mut names = NameIn::on(&root).unwrap();
        // Nothing heard: nothing, at once.
        assert_eq!(names.newest(5_000_000), None);
        let mut says = bus
            .publisher_declared(NameHeard::TOPIC, &NameHeard::declaration(), "name")
            .unwrap();
        let heard = |score, ago_ms| NameHeard { score, ago_ms };
        says.publish_json(1, &heard(0.95, 400)).unwrap();
        let since = std::time::Instant::now();
        let mut now = 9_000_000;
        let began = loop {
            now += NAME_LOOK_EVERY_US;
            if let Some(began) = names.newest(now) {
                break began;
            }
            assert!(since.elapsed() < WAIT * 4);
            std::thread::sleep(Duration::from_millis(2));
        };
        // 400 ms before it was taken up.
        assert_eq!(began, now - 400_000);
        // Two that came together: the one that began latest.
        says.publish_json(2, &heard(0.91, 900)).unwrap();
        says.publish_json(3, &heard(0.99, 100)).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        // Asked again within 20 ms of the last look: the bus is not
        // looked at, and nothing is lost by that.
        assert_eq!(names.newest(now + NAME_LOOK_EVERY_US - 1), None);
        assert_eq!(names.newest_scored(20_000_000), Some((19_900_000, 0.99)));
        assert_eq!(names.newest(21_000_000), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_head_tap_is_taken_from_the_bus_once_while_fresh_and_nothing_else_is() {
        // #711: only Controls' head Tap asks the lamp to listen.
        use lamp_msgs::{TouchEvent, TouchGesture, TouchIntent};
        let (bus, root) = bus("taps");
        let mut taps = TapsIn::on(&root).unwrap();
        assert_eq!(taps.newest(5_000_000), None);
        let mut says = bus
            .publisher_declared(TouchEvent::TOPIC, &TouchEvent::declaration(), "controls")
            .unwrap();
        let now = lamp_ipc::monotonic_us();
        let (_, touches) = crate::hand_controls::test_head_touch("boot", now - 400_000);
        let touch = touches[0].clone();
        let mut tap = touch.clone();
        tap.gesture = TouchGesture::Tap;
        (tap.when_speaking, tap.when_quiet) = (TouchIntent::Attend, TouchIntent::Attend);
        tap.when_asleep = Some(TouchIntent::Wake);
        tap.released_us = Some(tap.began_us + 200_000);
        tap.observed_us = tap.began_us + 350_000;
        tap.stamp.at_us = tap.observed_us;
        tap.stamp.sequence += 1;
        (tap.active_mask, tap.active_names) = (0, Vec::new());
        tap.check().unwrap();
        // A touch alone is not a tap; the tap is taken, dated by its release.
        says.publish_json(1, &touch).unwrap();
        says.publish_json(2, &tap).unwrap();
        let since = std::time::Instant::now();
        let mut look = now;
        let tapped = loop {
            look += NAME_LOOK_EVERY_US;
            if let Some(tapped) = taps.newest(look) {
                break tapped;
            }
            assert!(since.elapsed() < WAIT * 4);
            std::thread::sleep(Duration::from_millis(2));
        };
        assert_eq!(tapped, tap.released_us.unwrap());
        // The same tap again is not taken twice.
        says.publish_json(3, &tap).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(taps.newest(look + NAME_LOOK_EVERY_US), None);
        // A tap read more than two seconds after it was let go is not taken.
        let mut old = tap.clone();
        old.stamp.sequence += 1;
        says.publish_json(4, &old).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        let late = tap.released_us.unwrap() + TAP_FRESH_US;
        assert_eq!(taps.newest(late.max(look + 2 * NAME_LOOK_EVERY_US)), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// The reviewer on #1107: a head tap whose hand arrived while the lamp
    /// was speaking stopped it, and is no call, so the thump or a
    /// colleague's line in the next two seconds is not taken as said to the
    /// lamp. One whose hand came once the lamp was quiet still is a call.
    #[test]
    fn a_head_tap_that_stopped_the_lamps_speech_is_no_call() {
        use lamp_msgs::{TouchEvent, TouchGesture, TouchIntent};
        let (bus, root) = bus("tap-stop");
        let mut taps = TapsIn::on(&root).unwrap();
        let mut voice = bus
            .publisher_declared(VoiceState::TOPIC, &VoiceState::declaration(), SOURCE)
            .unwrap();
        let mut says = bus
            .publisher_declared(TouchEvent::TOPIC, &TouchEvent::declaration(), "controls")
            .unwrap();
        let now = lamp_ipc::monotonic_us();
        let tap_from = |began: u64, sequence: u64| {
            let (_, touches) = crate::hand_controls::test_head_touch("boot", began);
            let mut tap = touches[0].clone();
            tap.gesture = TouchGesture::Tap;
            (tap.when_speaking, tap.when_quiet) = (TouchIntent::Attend, TouchIntent::Attend);
            tap.when_asleep = Some(TouchIntent::Wake);
            tap.released_us = Some(tap.began_us + 200_000);
            tap.observed_us = tap.began_us + 350_000;
            tap.stamp.at_us = tap.observed_us;
            tap.stamp.sequence += sequence;
            (tap.active_mask, tap.active_names) = (0, Vec::new());
            tap.check().unwrap();
            tap
        };
        let look_until = |taps: &mut TapsIn, from: u64, until: u64| {
            let mut look = from;
            let mut taken = None;
            while look < until && taken.is_none() {
                look += NAME_LOOK_EVERY_US;
                taken = taps.newest(look);
                std::thread::sleep(Duration::from_millis(2));
            }
            taken
        };
        // Speaking when the hand arrives: the tap is no call.
        voice
            .publish_json(
                1,
                &VoiceState {
                    state: Voice::Speaking,
                },
            )
            .unwrap();
        assert_eq!(look_until(&mut taps, now - 600_000, now - 500_000), None);
        says.publish_json(1, &tap_from(now - 450_000, 1)).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(look_until(&mut taps, now, now + 200_000), None);
        // Quiet, a tap a second later is a call.
        voice
            .publish_json(
                2,
                &VoiceState {
                    state: Voice::Listening,
                },
            )
            .unwrap();
        let later = tap_from(now + 1_000_000, 2);
        says.publish_json(2, &later).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(
            look_until(&mut taps, now + 1_300_000, now + 1_600_000),
            later.released_us
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn what_the_microphone_hears_goes_out_twenty_milliseconds_to_a_message() {
        use lamp_msgs::Audio;
        let (bus, root) = bus("mic");
        let mut hears = bus
            .subscribe_typed("mic2/audio", "test", Audio::TYPE)
            .unwrap();
        let mut out = MicOut::on(&bus, "mic2").unwrap();
        assert_eq!(
            bus.declaration("mic2/audio").unwrap(),
            Some(Audio::declaration())
        );
        // Five blocks of 10 ms, each a ramp of its own: two messages, and
        // half of a third kept for the next block.
        for block in 0..5i16 {
            let samples: Vec<i16> = (0..160).map(|n| block * 1_000 + n - 80).collect();
            out.block(&samples, 10_000 * u64::from(block as u16));
        }
        for expected in 0..2u64 {
            let message = hears.recv(WAIT).unwrap().expect("a message");
            assert_eq!(message.header.enc, Audio::ENCODING);
            assert_eq!(message.header.source, "voice");
            let (header, samples) = Audio::split(&message.payload).unwrap();
            assert_eq!(
                header,
                Audio {
                    sample_rate: 16_000,
                    channels: 1,
                    sample_format: "S16LE".into(),
                    sequence: expected,
                }
            );
            assert_eq!(samples.len(), 640);
            // The samples as they were, low byte first, in order.
            let first = i16::from_le_bytes([samples[0], samples[1]]);
            let last = i16::from_le_bytes([samples[638], samples[639]]);
            let block = expected as i16 * 2;
            assert_eq!(
                (first, last),
                (block * 1_000 - 80, (block + 1) * 1_000 + 79)
            );
        }
        assert!(hears.recv(Duration::from_millis(50)).unwrap().is_none());
        out.block(&[7; 160], 60_000);
        let third = hears.recv(WAIT).unwrap().expect("the third");
        assert_eq!(Audio::split(&third.payload).unwrap().0.sequence, 2);
        // A microphone the bus has no topic for is refused, and with the
        // bus gone nothing fails.
        assert!(MicOut::on(&bus, "mic3").is_err());
        std::fs::remove_dir_all(root).unwrap();
        out.block(&[0; 320], 80_000);
        out.tick(2_000_000);
    }

    #[test]
    fn what_was_heard_and_what_was_said_go_to_their_own_topics() {
        let (bus, root) = bus("speech");
        let mut out = BusOut::on(&bus).unwrap();
        let mut heard = bus
            .subscribe_typed("voice/heard", "h", Speech::TYPE)
            .unwrap();
        let mut said = bus
            .subscribe_typed("voice/said", "s", Speech::TYPE)
            .unwrap();
        // Before any turn's input began, then in turn 7's, then in 8's.
        out.heard("Hm.", true, 1);
        out.input_started(7);
        out.heard("How do I take my coffee?", true, 2);
        out.said(7, "Black.", false, 3);
        out.input_started(8);
        out.heard("Thanks.", true, 4);
        let before: Speech = heard.recv(WAIT).unwrap().unwrap().parse().unwrap();
        assert_eq!((before.text.as_str(), before.request), ("Hm.", None));
        let person: Speech = heard.recv(WAIT).unwrap().unwrap().parse().unwrap();
        assert_eq!(
            (person.text.as_str(), person.finished, person.request),
            ("How do I take my coffee?", true, Some(7))
        );
        let next: Speech = heard.recv(WAIT).unwrap().unwrap().parse().unwrap();
        assert_eq!((next.text.as_str(), next.request), ("Thanks.", Some(8)));
        let lamp: Speech = said.recv(WAIT).unwrap().unwrap().parse().unwrap();
        assert_eq!(
            (lamp.text.as_str(), lamp.finished, lamp.request),
            ("Black.", false, Some(7))
        );
        assert!(heard.recv(Duration::from_millis(30)).unwrap().is_none());
        assert!(said.recv(Duration::from_millis(30)).unwrap().is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    /// A function call written as words is never on `voice/said`: an answer
    /// opening with a name has none of its words published, a later call
    /// is left out in however many pieces it came, and the next answer
    /// publishes again (lamp-4ace, 2026-10-16 17:12; Second on #1139).
    #[test]
    fn a_function_call_written_as_words_is_never_published_as_said() {
        let (bus, root) = bus("calls");
        let mut out = BusOut::on(&bus).unwrap();
        let mut said = bus
            .subscribe_typed("voice/said", "s", Speech::TYPE)
            .unwrap();
        out.said(1, "not_for_me", false, 1);
        out.said(1, " Sure.", true, 2);
        out.said(2, "Yes?", false, 3);
        out.said(2, " not_for", false, 4);
        out.said(2, "_me{}", false, 5);
        out.said(2, " Noon.", true, 6);
        let mut texts = Vec::new();
        while let Some(message) = said.recv(Duration::from_millis(100)).unwrap() {
            let speech: Speech = message.parse().unwrap();
            texts.push((speech.request, speech.text));
        }
        assert_eq!(
            texts,
            [(Some(2), "Yes?".to_owned()), (Some(2), " Noon.".to_owned())]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_silent_stream_is_not_taken_for_a_voice_that_has_gone() {
        let root = std::path::PathBuf::from(format!("/tmp/lvb-alive-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap().lease(Duration::from_millis(300));
        let mut out = BusOut::on(&bus).unwrap();
        let mut heard = bus
            .subscribe_typed("voice/heard", "h", Speech::TYPE)
            .unwrap();
        // Nothing is said for four leases, and the state is told as it
        // would be once a second on a lamp, here every 100 ms.
        for tick in 0..12u64 {
            out.state(None, tick * REPEAT_US);
            std::thread::sleep(Duration::from_millis(100));
            assert!(heard.recv(Duration::from_millis(1)).unwrap().is_none());
            assert!(!heard.source_gone(), "tick {tick}");
        }
        // Without it, the lease runs out.
        std::thread::sleep(Duration::from_millis(700));
        let _ = heard.recv(Duration::from_millis(1)).unwrap();
        assert!(heard.source_gone());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_room_is_published_when_it_changes_and_every_second_and_the_worker_is_given_the_root_by_a_flag()
     {
        use crate::room_told::Room as Judged;
        let (bus, root) = bus("room");
        let mut out = RoomOut::on(&root).unwrap();
        let mut watch = bus
            .subscribe_typed(RoomState::TOPIC, "watch", RoomState::TYPE)
            .unwrap();
        let mut seen = Vec::new();
        for (room, at_us) in [
            (Judged::Unknown, 0),
            // Within the second: not said again.
            (Judged::Unknown, 900_000),
            // A change, at once.
            (Judged::Busy, 950_000),
            (Judged::Busy, 1_900_000),
            // A second on with no change: said again, for a reader that
            // takes an old judgement as none.
            (Judged::Busy, 1_950_000),
            (Judged::Quiet, 2_000_000),
        ] {
            out.tell(room, at_us);
            while let Some(message) = watch.recv(Duration::from_millis(30)).unwrap() {
                seen.push(message.parse::<RoomState>().unwrap().state);
            }
        }
        assert_eq!(seen, [Room::Unknown, Room::Busy, Room::Busy, Room::Quiet]);
        std::fs::remove_dir_all(root).unwrap();

        assert!(worker_arguments(None, None).is_empty());
        assert!(worker_arguments(Some("yes"), Some("/x")).is_empty());
        assert_eq!(
            worker_arguments(Some("on"), None),
            [ROOT_FLAG, "/run/lampos-bus"]
        );
        assert_eq!(
            worker_arguments(Some("on"), Some("relative")),
            [ROOT_FLAG, "/run/lampos-bus"]
        );
        assert_eq!(
            worker_arguments(Some("on"), Some("/tmp/b")),
            [ROOT_FLAG, "/tmp/b"]
        );
        let flags = [ROOT_FLAG.to_owned(), "/tmp/b".to_owned(), "rest".to_owned()];
        let (taken, rest) = take_root(&flags).unwrap();
        assert_eq!((taken, rest), (Some(PathBuf::from("/tmp/b")), &flags[2..]));
        let (taken, rest) = take_root(&flags[2..]).unwrap();
        assert_eq!((taken, rest), (None, &flags[2..]));
        assert!(take_root(&[ROOT_FLAG.to_owned()]).is_err());
        assert!(take_root(&[ROOT_FLAG.to_owned(), "relative".to_owned()]).is_err());
    }

    #[test]
    fn with_nobody_listening_and_with_the_bus_gone_nothing_fails() {
        let (bus, root) = bus("gone");
        let mut out = BusOut::on(&bus).unwrap();
        out.state(None, 0);
        out.heard("anyone?", true, 1);
        std::fs::remove_dir_all(&root).unwrap();
        out.state(Some(RingPhase::Speaking), 2);
        out.said(1, "still talking", true, 3);
    }
}
