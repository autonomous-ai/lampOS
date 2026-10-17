//! What the lamp's first topics carry. See `docs/msgs.md`.
//!
//! ROS gives every topic one message type, and both ends "must agree on the
//! layout". The lamp's bus holds a publisher and a subscriber to a type's
//! name (`docs/bus.md`); this crate is where the types behind the names are,
//! so that the two ends share one definition and not two copies of it.
//!
//! Each type is a plain structure sent as JSON, with the name the bus
//! compares (`TYPE`), a version (`VERSION`), the topic or topics it travels
//! on, and that topic's declaration: its kind and how often it is promised.
//! Where ROS has the type, the fields have ROS's names and units: SI units,
//! angles in radians, times in seconds.
//!
//! Three rules hold for every type here:
//!
//! - A field this version does not know is ignored, so that a newer
//!   publisher does not break an older subscriber.
//! - A field that may be absent is an `Option` and is left out when there is
//!   nothing to say. Every other field has to be there.
//! - A value of an enumerated field that this version does not know is an
//!   error for that message: a state cannot be guessed. Adding one means a
//!   new `VERSION`.
//!
//! ```no_run
//! use lamp_bus::Bus;
//! use lamp_msgs::JointState;
//!
//! let bus = Bus::from_environment()?;
//! let mut joints = bus.publisher_declared(JointState::TOPIC, &JointState::declaration(), "body")?;
//! joints.publish_json(98_123_456, &JointState {
//!     name: vec!["yaw".into()],
//!     position: vec![0.12],
//!     velocity: vec![],
//!     effort: vec![],
//!     commanded: vec![0.15],
//! })?;
//!
//! let mut follow = bus.subscribe_typed(JointState::TOPIC, "follow", JointState::TYPE)?;
//! if let Some(message) = follow.recv(std::time::Duration::from_millis(100))? {
//!     let state: JointState = message.parse()?;
//!     println!("{} at {} rad", state.name[0], state.position[0]);
//! }
//! # Ok::<(), std::io::Error>(())
//! ```
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt, io, time::Duration};

// Transport metadata is shared with the bus CLI, which cannot depend back on
// this crate. The canonical schema is defined once in lamp_bus::logging.
pub use lamp_bus::logging::{Log, LogLevel};

mod expression;
pub use expression::*;
mod controls;
pub use controls::*;
mod ears;
pub use ears::*;
mod air;
pub use air::*;
mod schedule;
pub use schedule::*;
mod mood;
pub use mood::*;
mod attention;
pub use attention::*;
mod speaker_check;
mod tasks;
pub use speaker_check::*;
pub use tasks::*;
mod selftest;
pub use selftest::*;
mod skills;
pub use skills::*;
mod online;
pub use online::*;
mod arrival;
pub use arrival::*;
mod skill_ask;
pub use skill_ask::*;
mod arbitration;
pub use arbitration::*;
mod privacy;
pub use privacy::*;

/// Why a message is not one its type allows, in words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invalid(pub String);

impl fmt::Display for Invalid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Invalid {}

fn invalid<T>(why: String) -> Result<T, Invalid> {
    Err(Invalid(why))
}

/// One block of sound from a microphone (`mic1/audio`, `mic2/audio`).
///
/// This is the block's header only. The samples are not JSON: a message's
/// payload is this header as one line, a line end, then the samples as they
/// are (`Audio::block`, `Audio::split`), under the encoding
/// [`Audio::ENCODING`]. The first three fields are ROS's
/// `audio_common_msgs/AudioInfo`, which ROS sends once on a topic of its
/// own; here they travel with every block, so that a block read alone, or
/// from a recording, says what it is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Audio {
    /// Samples a second for each channel, in hertz.
    pub sample_rate: u32,
    /// How many channels are interleaved in the block.
    pub channels: u8,
    /// How one sample is written: [`Audio::S16LE`] is signed 16 bits, low
    /// byte first.
    pub sample_format: String,
    /// The block's count since the microphone's owner started, from 0. A
    /// step of more than one between two blocks is sound that was lost.
    pub sequence: u64,
}

impl Audio {
    pub const TYPE: &str = "lamp_msgs/Audio";
    pub const VERSION: u32 = 1;
    pub const TOPICS: [&str; 2] = ["mic1/audio", "mic2/audio"];
    /// The bus's label for a payload made by [`Audio::block`].
    pub const ENCODING: &str = "json+pcm";
    pub const S16LE: &str = "S16LE";

    /// A stream: each block is over when it has been sent. One every 20 ms.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event).every(Duration::from_millis(20))
    }

    /// A message's payload: this header, a line end, then `samples`.
    pub fn block(&self, samples: &[u8]) -> io::Result<Vec<u8>> {
        let mut payload = serde_json::to_vec(self)?;
        payload.push(b'\n');
        payload.extend_from_slice(samples);
        Ok(payload)
    }

    /// The header and the samples of a payload made by [`Audio::block`].
    pub fn split(payload: &[u8]) -> io::Result<(Self, &[u8])> {
        let Some(end) = payload.iter().position(|&byte| byte == b'\n') else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "no line end after the audio header",
            ));
        };
        Ok((
            serde_json::from_slice(&payload[..end])?,
            &payload[end + 1..],
        ))
    }
}

/// Where the camera's newest picture is (`camera/image`). The picture
/// stays in a file its publisher keeps: it is too large for a message.
///
/// `width`, `height` and `encoding` are ROS's `sensor_msgs/Image` fields,
/// and `stamp` is its `header.stamp`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageRef {
    /// The file that holds the frame: an absolute path.
    pub path: String,
    /// Columns, in pixels.
    pub width: u32,
    /// Rows, in pixels.
    pub height: u32,
    /// How the file is to be read: `jpeg`, or a ROS encoding such as
    /// `rgb8`.
    pub encoding: String,
    /// When the frame was captured, not when this was sent: seconds since
    /// 1970.
    pub stamp: f64,
}

impl ImageRef {
    pub const TYPE: &str = "lamp_msgs/ImageRef";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "camera/image";

    /// The newest picture replaces the one before. At least two a second.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest).every(Duration::from_millis(500))
    }

    /// The path has to mean the same file to every process.
    pub fn check(&self) -> Result<(), Invalid> {
        if !self.path.starts_with('/') {
            return invalid(format!("image path {:?} is not absolute", self.path));
        }
        if self.width == 0 || self.height == 0 {
            return invalid(format!(
                "an image of {} by {} has no pixels",
                self.width, self.height
            ));
        }
        Ok(())
    }
}

/// Where every joint is (`body/joint_states`): ROS's
/// `sensor_msgs/JointState`, and what each joint was last asked to be.
///
/// As in ROS, every value in one message is of the same moment, and each
/// list is as long as `name` or empty: a body that cannot measure effort
/// leaves `effort` empty.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JointState {
    /// The joints, in the order of every other list.
    pub name: Vec<String>,
    /// Where each joint is, in radians.
    pub position: Vec<f64>,
    /// How fast each is turning, in radians a second.
    pub velocity: Vec<f64>,
    /// How hard each is working: the share of its full strength its servo
    /// reports, from 0 to 1, without a direction. ROS's field is a torque
    /// in newton metres, which these servos do not report.
    pub effort: Vec<f64>,
    /// Where each was last asked to be, in radians. Not in ROS's type: this
    /// body knows it, and the difference from `position` is what a joint
    /// that is held or blocked shows.
    pub commanded: Vec<f64>,
}

impl JointState {
    pub const TYPE: &str = "lamp_msgs/JointState";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "body/joint_states";

    /// Where the joints are now replaces where they were. At least ten a
    /// second.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest).every(Duration::from_millis(100))
    }

    /// Each list is as long as `name`, or empty.
    pub fn check(&self) -> Result<(), Invalid> {
        let joints = self.name.len();
        for (list, length) in [
            ("position", self.position.len()),
            ("velocity", self.velocity.len()),
            ("effort", self.effort.len()),
            ("commanded", self.commanded.len()),
        ] {
            if length != 0 && length != joints {
                return invalid(format!(
                    "{list} has {length} values for {joints} joints: as many, or none"
                ));
            }
        }
        Ok(())
    }
}

/// One face in the camera's view.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Face {
    /// A passing id, as in ROS4HRI: the same for as long as this face stays
    /// in view, and nobody's once it has gone. It does not say who it is.
    pub id: String,
    /// The face's box in the picture, each edge as a fraction of the
    /// picture from 0 to 1, with (0, 0) the top left corner: ROS4HRI's
    /// `hri_msgs/NormalizedRegionOfInterest2D`.
    pub xmin: f32,
    pub ymin: f32,
    pub xmax: f32,
    pub ymax: f32,
    /// How sure the detector is that this is a face, from 0 to 1.
    pub confidence: f32,
    /// How far the head is turned to one side, in radians, if that was
    /// estimated: 0 when it faces the camera, and more to the face's own
    /// left, which is counter-clockwise seen from above as ROS has yaw.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yaw: Option<f32>,
    /// Whether the face is turned to the lamp, if that was judged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub facing: Option<bool>,
    /// Whether the face's mouth moves as a speaking mouth does, if the
    /// eyes were asked to judge it (`LAMP_EYES_SPEAKING=on`, docs/eyes.md,
    /// "Whether a face is speaking"). Absent when they were not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaking: Option<Speaking>,
}

/// A face's mouth, judged from its landmarks over the last second or so.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Speaking {
    /// `yes`, `no`, or `unknown` when the face is too small, turned, too
    /// new, or its landmarks jitter too much to tell.
    pub state: SpeakingState,
    /// How much the mouth moved beyond its landmarks' own jitter, in
    /// inter-ocular distances; 0 when `unknown`.
    pub score: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpeakingState {
    Yes,
    No,
    Unknown,
}

impl Face {
    /// The box lies in the picture and has a size, and the confidence is
    /// one.
    pub fn check(&self) -> Result<(), Invalid> {
        let unit = |value: f32| (0.0..=1.0).contains(&value);
        let edges = [self.xmin, self.ymin, self.xmax, self.ymax];
        if !edges.into_iter().all(unit) || self.xmin >= self.xmax || self.ymin >= self.ymax {
            return invalid(format!(
                "face {:?} has the box {edges:?}: each edge is from 0 to 1, min before max",
                self.id
            ));
        }
        if !unit(self.confidence) {
            return invalid(format!(
                "face {:?} has confidence {}: from 0 to 1",
                self.id, self.confidence
            ));
        }
        Ok(())
    }
}

/// The faces in view now (`humans/faces/tracked`, ROS4HRI's name for the
/// list of faces). An empty list says that the camera looked and saw none.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Faces {
    pub faces: Vec<Face>,
}

impl Faces {
    pub const TYPE: &str = "lamp_msgs/Faces";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "humans/faces/tracked";

    /// Who is in view now replaces who was. With each look: no period is
    /// promised.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
    }

    pub fn check(&self) -> Result<(), Invalid> {
        self.faces.iter().try_for_each(Face::check)
    }
}

/// What was heard, or what was said, as text (`voice/heard`,
/// `voice/said`). Modelled on ROS4HRI's `hri_msgs/LiveSpeech`, whose
/// `incremental` and `final` are here one `text` and whether it is
/// `finished`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Speech {
    pub text: String,
    /// False while the words are still coming and `text` may yet change,
    /// true for the whole of what was said.
    pub finished: bool,
    /// The language, as `en_GB`, if it is known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    /// The number of the request this belongs to, if it is known: what
    /// ties what was heard to what was said in answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<u64>,
}

impl Speech {
    pub const TYPE: &str = "lamp_msgs/Speech";
    pub const VERSION: u32 = 1;
    pub const TOPICS: [&str; 2] = ["voice/heard", "voice/said"];

    /// A stream: a thing said is over when it has been said. With each
    /// thing said: no period is promised.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
    }
}

/// What the voice is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Voice {
    Resting,
    Listening,
    Thinking,
    Speaking,
}

/// How the voice's health message begins while it waits with its
/// microphones off, by the privacy switch or a mute (#1371): `lamp-health
/// show` says it on its first line, so a muted lamp never reads as a quiet
/// one.
pub const MICROPHONES_OFF: &str = "the microphones are off";

/// What the voice is doing now (`voice/state`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceState {
    pub state: Voice,
}

impl VoiceState {
    pub const TYPE: &str = "lamp_msgs/VoiceState";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "voice/state";

    /// The state now replaces the one before. When it changes, and once a
    /// second.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest).every(Duration::from_secs(1))
    }
}

/// Whether the voice's provider can be reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Link {
    Ready,
    Unavailable,
}

/// Why the provider cannot be reached, as a kind of trouble and never as
/// the provider's own words.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkFault {
    /// Something took longer than it is given: connecting, setting up, an
    /// answer.
    Timeout,
    /// The provider does not accept the lamp's credentials.
    Authentication,
    /// The provider is refusing for now: a rate or a quota.
    Limited,
    /// The provider answered that it cannot serve.
    Server,
    /// The connection failed or was closed.
    Transport,
    /// The provider sent something the lamp does not accept.
    Protocol,
    /// The trouble is in the lamp's own process.
    Local,
}

/// Whether the voice's provider can be reached now (`voice/link`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceLink {
    pub state: Link,
    /// Why not, when it cannot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<LinkFault>,
}

impl VoiceLink {
    pub const TYPE: &str = "lamp_msgs/VoiceLink";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "voice/link";

    /// The link now replaces what it was. When it changes, and once a
    /// second.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest).every(Duration::from_secs(1))
    }

    pub fn ready() -> Self {
        Self {
            state: Link::Ready,
            reason: None,
        }
    }

    pub fn unavailable(reason: LinkFault) -> Self {
        Self {
            state: Link::Unavailable,
            reason: Some(reason),
        }
    }

    /// A link that is ready has no reason, and one that is not has one.
    pub fn check(&self) -> Result<(), Invalid> {
        match (self.state, self.reason) {
            (Link::Ready, None) | (Link::Unavailable, Some(_)) => Ok(()),
            (Link::Ready, Some(reason)) => invalid(format!(
                "a ready link has no reason, and this says {reason:?}"
            )),
            (Link::Unavailable, None) => invalid("an unavailable link says why".into()),
        }
    }
}

/// What the room sounds like.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Room {
    /// Not judged yet, or not to be judged just now.
    Unknown,
    Quiet,
    Busy,
}

/// What the room is like now (`room/state`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomState {
    pub state: Room,
}

impl RoomState {
    pub const TYPE: &str = "lamp_msgs/RoomState";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "room/state";

    /// The state now replaces the one before. When it changes: no period
    /// is promised, so its publisher says that it is alive between changes.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
    }
}

/// The lamp's own light: what is asked for (`light/command`) and what is
/// shown (`light/state`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Light {
    pub on: bool,
    /// One of [`Light::COLOURS`], by name.
    pub colour: String,
    /// How bright, from 1, the dimmest, to [`Light::BRIGHTEST`].
    pub level: u8,
}

impl Light {
    pub const TYPE: &str = "lamp_msgs/Light";
    pub const VERSION: u32 = 1;
    /// What is asked for, and what is shown.
    pub const TOPICS: [&str; 2] = ["light/command", "light/state"];
    /// The ten colours a light may be.
    pub const COLOURS: [&str; 10] = [
        "yellow", "red", "green", "blue", "cyan", "purple", "orange", "pink", "white", "warm",
    ];
    pub const BRIGHTEST: u8 = 5;

    /// The light now replaces the one before. When it changes, and once a
    /// second.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest).every(Duration::from_secs(1))
    }

    /// A light of a colour and a level that there are, or why not.
    pub fn new(on: bool, colour: &str, level: u8) -> Result<Self, Invalid> {
        let light = Self {
            on,
            colour: colour.into(),
            level,
        };
        light.check().map(|()| light)
    }

    pub fn is_colour(name: &str) -> bool {
        Self::COLOURS.contains(&name)
    }

    /// The colour is one of the ten and the level from 1 to 5, whether the
    /// light is on or off: a light that is off still has the colour it
    /// would be.
    pub fn check(&self) -> Result<(), Invalid> {
        if !Self::is_colour(&self.colour) {
            return invalid(format!(
                "{:?} is not a colour of the light: {}",
                self.colour,
                Self::COLOURS.join(", ")
            ));
        }
        if !(1..=Self::BRIGHTEST).contains(&self.level) {
            return invalid(format!(
                "level {} is not from 1 to {}",
                self.level,
                Self::BRIGHTEST
            ));
        }
        Ok(())
    }
}

/// How well a part is: ROS's `DiagnosticStatus` levels, by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Ok,
    Warn,
    Error,
    /// The part has not been heard from, said by whoever watches it.
    Stale,
}

/// One fact beside a part's health, both sides text as in ROS's
/// `diagnostic_msgs/KeyValue`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyValue {
    pub key: String,
    pub value: String,
}

/// One part's health (`diagnostics`): ROS's
/// `diagnostic_msgs/DiagnosticStatus` without `hardware_id`, of which one
/// lamp has one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub level: Level,
    /// The part this is about.
    pub name: String,
    /// What is the matter, for a person. Empty when nothing is.
    pub message: String,
    /// A few facts that explain the level.
    pub values: Vec<KeyValue>,
}

impl Diagnostic {
    pub const TYPE: &str = "lamp_msgs/Diagnostic";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "diagnostics";

    /// `name` is well: nothing is the matter and there is nothing to say.
    pub fn ok(name: &str) -> Self {
        Self::new(Level::Ok, name, "")
    }

    /// `name` is at `level`, and `message` says why, for a person.
    pub fn new(level: Level, name: &str, message: &str) -> Self {
        Self {
            level,
            name: name.to_owned(),
            message: message.to_owned(),
            values: Vec::new(),
        }
    }

    /// The same with one more fact beside it.
    #[must_use]
    pub fn with(mut self, key: &str, value: impl ToString) -> Self {
        self.values.push(KeyValue {
            key: key.to_owned(),
            value: value.to_string(),
        });
        self
    }

    /// A stream every part publishes to: one part's report does not
    /// replace another's. Once a second.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event).every(Duration::from_secs(1))
    }
}

impl Level {
    /// How bad, for putting the worst first: ok, stale, warn, error. A
    /// part that has gone quiet is worse than one that is well and not as
    /// bad as one that says something is wrong.
    pub fn rank(self) -> u8 {
        match self {
            Self::Ok => 0,
            Self::Stale => 1,
            Self::Warn => 2,
            Self::Error => 3,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warn => "warn",
            Self::Error => "error",
            Self::Stale => "stale",
        }
    }
}

/// One part's place in the lamp's health.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthItem {
    /// The part, as it named itself on `diagnostics`.
    pub name: String,
    pub level: Level,
    /// What is the matter, for a person. Empty when nothing is.
    pub message: String,
    /// How long ago it last reported, in milliseconds.
    pub age_ms: u64,
}

/// How the whole lamp is (`lamp/health`): every part's newest report put
/// together, as ROS's diagnostic aggregator does on `/diagnostics_agg`.
///
/// A part says how it is on `diagnostics` and never says of itself that
/// it is stale. Whoever writes this does: a part not heard from for five
/// seconds is `stale`, and a part that is expected and has never reported
/// is in `missing`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Health {
    /// The worst level among the items; `stale` at least if a part is
    /// missing.
    pub level: Level,
    /// The worst item's name and message, for a person: `body: yaw is
    /// hot`. Empty when all is well.
    pub message: String,
    /// Every part that has reported, the worst first.
    pub items: Vec<HealthItem>,
    /// Parts that are expected and have never reported.
    pub missing: Vec<String>,
}

impl Health {
    pub const TYPE: &str = "lamp_msgs/Health";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "lamp/health";

    /// How the lamp is now replaces how it was. When it changes and every
    /// five seconds, and it means nothing after fifteen: a health that
    /// nobody renews is not a healthy lamp.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
            .every(Duration::from_secs(5))
            .lasting(Duration::from_secs(15))
    }

    /// The level is the worst there is, and the items are worst first.
    pub fn check(&self) -> Result<(), Invalid> {
        let worst = self
            .items
            .iter()
            .map(|item| item.level)
            .chain((!self.missing.is_empty()).then_some(Level::Stale))
            .max_by_key(|level| level.rank())
            .unwrap_or(Level::Ok);
        if self.level != worst {
            return invalid(format!(
                "the level is {} and the worst of its parts is {}",
                self.level.as_str(),
                worst.as_str()
            ));
        }
        if self
            .items
            .windows(2)
            .any(|pair| pair[0].level.rank() < pair[1].level.rank())
        {
            return invalid("the items are not worst first".into());
        }
        Ok(())
    }
}

/// The computer the lamp runs on (`board/state`): the numbers, for
/// programs and plots. Whether a number is too high is not said here but
/// on `diagnostics`, by whoever publishes this.
///
/// What could not be measured is left out, never written as nothing: a
/// board with no Wi-Fi has no `wifi_dbm`, not one of 0.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoardState {
    /// Each temperature the board reports, by its sensor's name, in
    /// degrees Celsius.
    pub temperature_c: BTreeMap<String, f32>,
    /// The load over the last minute for each processor: 1.0 is every
    /// processor busy.
    pub cpu_load: f32,
    /// How much of the memory is in use, from 0 to 1.
    pub memory_used: f32,
    /// How much of the disk the system is on is free, from 0 to 1.
    pub disk_free: f32,
    /// Seconds since the board started.
    pub uptime_s: u64,
    /// The Wi-Fi signal, in dBm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wifi_dbm: Option<i32>,
    /// Whether the clock is set from the network.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clock_synced: Option<bool>,
}

impl BoardState {
    pub const TYPE: &str = "lamp_msgs/BoardState";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "board/state";

    /// The board now replaces the board before. Every five seconds, and
    /// it means nothing after fifteen.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
            .every(Duration::from_secs(5))
            .lasting(Duration::from_secs(15))
    }

    /// The fractions are fractions and nothing is not a number.
    pub fn check(&self) -> Result<(), Invalid> {
        for (name, value) in [
            ("memory_used", self.memory_used),
            ("disk_free", self.disk_free),
        ] {
            if !(0.0..=1.0).contains(&value) {
                return invalid(format!("{name} is {value}: from 0 to 1"));
            }
        }
        if !self.cpu_load.is_finite() || self.cpu_load < 0.0 {
            return invalid(format!("cpu_load is {}", self.cpu_load));
        }
        if let Some((name, value)) = self
            .temperature_c
            .iter()
            .find(|(_, value)| !value.is_finite())
        {
            return invalid(format!("temperature {name} is {value}"));
        }
        Ok(())
    }
}

/// The lamp's name was heard in what its microphone hears (`name/heard`):
/// "Hey Lamp" or "Lamp ơi", as a sound, by a part that listens for
/// nothing else. It is a sign that the lamp was called and never proof:
/// a name like the lamp's sets it off, and a call can be missed.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct NameHeard {
    /// How sure the listener was, from 0 to 1: the highest it reached
    /// while the name was being said.
    pub score: f32,
    /// How long before this was sent the name began to be heard, in
    /// milliseconds.
    pub ago_ms: u32,
}

impl NameHeard {
    pub const TYPE: &str = "lamp_msgs/NameHeard";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "name/heard";
    /// The same hearing's first sign (#782): said as soon as the score
    /// reaches the listener's bar, while the name is still being said,
    /// with the score so far, and `ago_ms` from when it reached it. Its
    /// final word is still on [`Self::TOPIC`], with the highest score. A
    /// first sign with no final word was a near miss.
    pub const RISING_TOPIC: &str = "name/rising";

    /// Each hearing is over when it has been sent. None is promised.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
    }

    /// The score is a share.
    pub fn check(&self) -> Result<(), Invalid> {
        if !(0.0..=1.0).contains(&self.score) {
            return invalid(format!("score is {}: from 0 to 1", self.score));
        }
        Ok(())
    }
}

/// Whether the lamp is paused (`lamp/pause`), as the keeper was last told
/// (`docs/keeper.md`, "Named states and the pause"). A paused part does no
/// new work and holds what it has; it is still running and ready.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pause {
    pub paused: bool,
}

impl Pause {
    pub const TYPE: &str = "lamp_msgs/Pause";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "lamp/pause";

    /// Said when it changes and kept until the next: a lamp is paused until
    /// it is resumed, or until the bus is made again at power-up.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
    }
}

/// Whether a hand is pushing the lamp now (`body/hand`), as the body feels
/// it: a head or base joint that met resistance and is held where it gave
/// way, for as long as it is held. `since_us` is the body's own clock
/// (microseconds since it started) when the push began, 0 when not pushed.
/// Said each time it changes and at least every 100 ms while the body runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hand {
    pub pushed: bool,
    pub since_us: u64,
}

impl Hand {
    pub const TYPE: &str = "lamp_msgs/Hand";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "body/hand";

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
    }
}

/// What became of a gesture (`body/gesture`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GestureState {
    /// The body began playing it.
    Began,
    /// It ended: played out, or let go as it played.
    Over,
    /// Asked of the body and dropped before it began (paused, held by a
    /// hand, busy, another playing, ...), or ended for a higher ask before
    /// it played out: no `over` follows.
    Dropped,
}

/// A gesture of the body's began, ended or was dropped (`body/gesture`),
/// said where lamp-body's journal says `gesture NAME`, `gesture over` and
/// `gesture NAME dropped: ...`. Read-only: nothing on the bus asks for a
/// gesture here. `name` is the clip's (`nod`, `dance_groove`, the body's
/// own `idle` and its shifts).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BodyGesture {
    pub name: String,
    pub state: GestureState,
}

impl BodyGesture {
    pub const TYPE: &str = "lamp_msgs/BodyGesture";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "body/gesture";

    /// Each one is news once.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
    }
}

/// Why the voice switched its microphones off (#711). None is the mute as it
/// always was: a person's privacy, cleared only by a fresh hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MuteReason {
    /// A hand kept three seconds on the head put the lamp to sleep; a tap on
    /// the head wakes it, as the usual unmute does.
    Sleep,
}

/// The voice's software mute (`voice/mute`, #711): whether its microphones
/// are switched off, and why. With `LAMP_HEAD_TOUCH=on` and
/// `--controls-mute`, the voice says it when it changes and every second, in
/// a conversation and in the wait while muted. Expression shows sleep from
/// it. The physical slide is not in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceMute {
    pub muted: bool,
    /// Only while muted; absent is the mute's present meaning.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<MuteReason>,
}

impl VoiceMute {
    pub const TYPE: &str = "lamp_msgs/VoiceMute";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "voice/mute";

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
            .every(Duration::from_secs(1))
            .lasting(Duration::from_secs(3))
    }

    /// A reason belongs only to a mute.
    pub fn check(&self) -> Result<(), Invalid> {
        if !self.muted && self.reason.is_some() {
            return Err(Invalid("an unmuted voice has no mute reason".into()));
        }
        Ok(())
    }

    /// Muted because the lamp was put to sleep.
    pub fn asleep(&self) -> bool {
        self.muted && self.reason == Some(MuteReason::Sleep)
    }
}

/// What the people at the lamp asked it about speaking up, as typed facts
/// for the policy (`memory/rules`, `docs/memory-days.md`, "Rules"). The
/// voice says it when a session opens for someone and when one of their
/// rules is kept or forgotten; it is kept until the next. Only booleans
/// and a count: never the words of a rule.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rules {
    /// Whose rules, beside the room's: the memory's person id (`p-` and
    /// 16 hex digits) of the known person at the lamp, or `everyone` when
    /// nobody was known.
    pub person: String,
    /// Asked never to speak first, unprompted.
    pub quiet: bool,
    /// Asked not to speak first, unprompted, in the mornings (before noon,
    /// the lamp's local time).
    pub quiet_mornings: bool,
    /// How many rules, of every kind, the room's and theirs.
    pub rules: u32,
    /// The greeting on arrival (#883), from rules about it. Absent from a
    /// message of before #813's typed rules: nothing held.
    #[serde(default)]
    pub hello: Unprompted,
    /// News the lamp brings unasked: progress and results of work (#765).
    #[serde(default)]
    pub news: Unprompted,
}

/// When the lamp was asked not to say one kind of thing unprompted, every
/// such rule merged, the strictest winning. `quiet` and `quiet_mornings`
/// are not copied in: they stand for both kinds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unprompted {
    /// Not at all.
    pub never: bool,
    /// Not before this minute of the day, local time (540 = 09:00).
    pub not_before_minute: Option<u16>,
    /// Not from this minute of the day on, local time (1320 = 22:00).
    pub not_from_minute: Option<u16>,
    /// Not while the person is talking: on a call, or with someone.
    pub not_while_talking: bool,
}

impl Rules {
    pub const TYPE: &str = "lamp_msgs/Rules";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "memory/rules";

    /// Said when it changes and kept until the next, like the pause.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
    }
}

/// The body's topics beside the first ones.
pub fn body_topics() -> Vec<(&'static str, Declaration)> {
    vec![
        (Hand::TOPIC, Hand::declaration()),
        (BodyGesture::TOPIC, BodyGesture::declaration()),
    ]
}

/// The voice's mute and its reason (#711), beside the first topics.
pub fn mute_topics() -> Vec<(&'static str, Declaration)> {
    vec![(VoiceMute::TOPIC, VoiceMute::declaration())]
}

/// One thing the policy decided (`policy/decisions`): what the question
/// was about, what was chosen, how likely the policy held it to be right,
/// and what each judgement behind it was.
///
/// The policy is the one part that chooses what the lamp does. It asks a
/// decision model questions that are answered with probabilities, combines
/// the answers, and says here what it chose (`docs/policy.md`). What was
/// heard is not in this message: it says what was decided, not about what
/// words.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    /// Which decision this is, by name: `attention` is "is this said to
    /// the lamp?", `progress` "say this progress now?", `hello` "say hello
    /// now?".
    pub about: String,
    /// What was chosen, by the decision's own names: for `attention`,
    /// `answer`, `look_up` or `stay_quiet`; for `progress` and `hello`,
    /// `say_now` or `wait`.
    pub choice: String,
    /// How likely the policy holds it that the lamp should act, from 0 to
    /// 1: what the choice was made from.
    pub probability: f32,
    /// What the decision model answered to each question, by the
    /// question's name: the probability of yes, from 0 to 1. Empty when no
    /// model answered and a rule decided.
    pub answers: BTreeMap<String, f32>,
    /// Who decided: the decision model's name as its service reported it,
    /// or `rule` when no model answered.
    pub by: String,
    /// How long the asking took, in milliseconds, whether it was answered
    /// or not.
    pub took_ms: u32,
    /// True while nothing acts on the decision: it is made and written
    /// down so that it can be compared with what the lamp did.
    pub shadow: bool,
    /// The number of the request this is about, if it is known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<u64>,
    /// The work this is about, for `progress` ("say this progress now?"):
    /// `<store>:<task>:<change>@<run>`, the tasks part's store, the task's
    /// number in it, the count of changes of its progress line that was
    /// judged, and the run of the tasks part that counted it
    /// (`tasks/status`): counts begin again at each run. Not there for any
    /// other decision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    /// The arrival this is about, for `hello` ("say hello now?"): the
    /// number `voice/arrival` gave it (`VoiceArrival::arrival`). Not there
    /// for any other decision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arrival: Option<u64>,
    /// The skill's ask this answers (#939): the number `skills/ask` gave it
    /// (`SkillAsk::ask`). Not there for any other decision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask: Option<u64>,
}

impl Decision {
    pub const TYPE: &str = "lamp_msgs/Decision";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "policy/decisions";

    /// A stream: a decision is over when it has been made. With each thing
    /// decided: no period is promised, so its publisher says that it is
    /// alive between decisions.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
    }

    /// The probability and every answer are probabilities, and the work
    /// named is `<store>:<task>:<change>@<run>`.
    pub fn check(&self) -> Result<(), Invalid> {
        let unit = |value: f32| (0.0..=1.0).contains(&value);
        if !unit(self.probability) {
            return invalid(format!(
                "a decision's probability is {}: from 0 to 1",
                self.probability
            ));
        }
        if let Some(task) = &self.task {
            let parts: Vec<&str> = task.split(':').collect();
            let number = |part: &str| {
                !part.is_empty() && part.len() <= 20 && part.bytes().all(|b| b.is_ascii_digit())
            };
            let store = |part: &str| {
                part.len() == 32
                    && part
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            };
            let change_of_run = parts
                .get(2)
                .and_then(|part| part.split_once('@'))
                .is_some_and(|(change, run)| number(change) && number(run));
            if parts.len() != 3 || !store(parts[0]) || !number(parts[1]) || !change_of_run {
                return invalid(
                    "a decision's task is <32 hexadecimal characters>:<task>:<change>@<run>".into(),
                );
            }
        }
        match self.answers.iter().find(|(_, answer)| !unit(**answer)) {
            Some((question, answer)) => {
                invalid(format!("the answer to {question} is {answer}: from 0 to 1"))
            }
            None => Ok(()),
        }
    }
}

/// Every topic that has a type here, with its declaration, by name: what
/// `docs/bus.md` lists under "The first topics", and the topics added
/// since, which are in the table of `docs/msgs.md`.
pub fn first_topics() -> Vec<(&'static str, Declaration)> {
    let mut topics = vec![
        (ImageRef::TOPIC, ImageRef::declaration()),
        (JointState::TOPIC, JointState::declaration()),
        (Faces::TOPIC, Faces::declaration()),
        (VoiceState::TOPIC, VoiceState::declaration()),
        (VoiceLink::TOPIC, VoiceLink::declaration()),
        (RoomState::TOPIC, RoomState::declaration()),
        (Diagnostic::TOPIC, Diagnostic::declaration()),
        (Health::TOPIC, Health::declaration()),
        (BoardState::TOPIC, BoardState::declaration()),
        (NameHeard::TOPIC, NameHeard::declaration()),
        (Decision::TOPIC, Decision::declaration()),
    ];
    topics.extend(Audio::TOPICS.map(|topic| (topic, Audio::declaration())));
    topics.extend(Speech::TOPICS.map(|topic| (topic, Speech::declaration())));
    topics.extend(Light::TOPICS.map(|topic| (topic, Light::declaration())));
    topics.sort_by_key(|(topic, _)| *topic);
    topics
}

/// How `topic` is declared, if it is one of the first topics.
/// The keeper's topic, separate from the original seventeen.
pub fn keeper_topics() -> Vec<(&'static str, Declaration)> {
    vec![(Pause::TOPIC, Pause::declaration())]
}

/// The memory's typed rules (#813), separate from the original seventeen.
pub fn memory_topics() -> Vec<(&'static str, Declaration)> {
    vec![(Rules::TOPIC, Rules::declaration())]
}

/// The first sign of each hearing of the lamp's name (`name/rising`, #782),
/// separate from the original seventeen, where its final word is.
pub fn name_topics() -> Vec<(&'static str, Declaration)> {
    vec![(NameHeard::RISING_TOPIC, NameHeard::declaration())]
}

pub fn declaration_of(topic: &str) -> Option<Declaration> {
    first_topics()
        .into_iter()
        .chain(expression_topics())
        .chain(controls_topics())
        .chain(touch_topics())
        .chain(schedule_topics())
        .chain(keeper_topics())
        .chain(body_topics())
        .chain(arbitration_topics())
        .chain(name_topics())
        .chain(mute_topics())
        .chain(tasks_topics())
        .chain(logging_topics())
        .chain(memory_topics())
        .chain(skills_topics())
        .chain(online_topics())
        .chain(arrival_topics())
        .chain(skill_ask_topics())
        .chain(privacy_topics())
        .find_map(|(name, declaration)| (name == topic).then_some(declaration))
}

/// Optional operational logs; the original first-topic inventory stays intact.
pub fn logging_topics() -> Vec<(&'static str, Declaration)> {
    vec![(Log::TOPIC, Log::declaration())]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::DeserializeOwned;
    use serde_json::{Value, json};

    /// `value` is written as exactly `text`, reads back as itself, and
    /// reads the same with a field no version yet knows beside the others.
    fn round_trip<T>(value: &T, text: &str)
    where
        T: Serialize + DeserializeOwned + PartialEq + fmt::Debug,
    {
        assert_eq!(serde_json::to_string(value).unwrap(), text);
        assert_eq!(&serde_json::from_str::<T>(text).unwrap(), value);
        let mut later: Value = serde_json::from_str(text).unwrap();
        later["added_later"] = json!({"by": "a newer publisher", "n": [1, 2]});
        assert_eq!(&serde_json::from_value::<T>(later).unwrap(), value);
    }

    /// `text` with one field taken out is not a `T`.
    fn needs<T: DeserializeOwned>(text: &str, field: &str) {
        let mut without: Value = serde_json::from_str(text).unwrap();
        assert!(without.as_object_mut().unwrap().remove(field).is_some());
        let error = serde_json::from_value::<T>(without).err().expect("refused");
        assert!(error.to_string().contains(field), "{error}");
    }

    #[test]
    fn audio_is_a_header_line_and_then_the_samples_as_they_are() {
        let audio = Audio {
            sample_rate: 16_000,
            channels: 1,
            sample_format: Audio::S16LE.into(),
            sequence: 412,
        };
        let text = "{\"sample_rate\":16000,\"channels\":1,\"sample_format\":\"S16LE\",\
                    \"sequence\":412}";
        round_trip(&audio, text);
        needs::<Audio>(text, "sequence");
        // Twenty milliseconds of sound, every byte value in it, line ends
        // and zeros too.
        let samples: Vec<u8> = (0..640u32).map(|index| (index * 7) as u8).collect();
        let payload = audio.block(&samples).unwrap();
        assert!(payload.starts_with(text.as_bytes()));
        assert_eq!(payload.len(), text.len() + 1 + 640);
        let (header, rest) = Audio::split(&payload).unwrap();
        assert_eq!((header, rest), (audio.clone(), &samples[..]));
        // No samples at all is a block too. No line end is not one.
        let bare = audio.block(&[]).unwrap();
        assert!(Audio::split(&bare).unwrap().1.is_empty());
        assert!(Audio::split(text.as_bytes()).is_err());
        assert!(Audio::split(b"{}\nxx").is_err());
    }

    #[test]
    fn an_image_reference_names_a_file_every_process_can_find() {
        let image = ImageRef {
            path: "/run/lampos-eyes/sight.jpg".into(),
            width: 640,
            height: 480,
            encoding: "jpeg".into(),
            stamp: 1_791_878_400.125,
        };
        let text = "{\"path\":\"/run/lampos-eyes/sight.jpg\",\"width\":640,\"height\":480,\
                    \"encoding\":\"jpeg\",\"stamp\":1791878400.125}";
        round_trip(&image, text);
        needs::<ImageRef>(text, "stamp");
        assert_eq!(image.check(), Ok(()));
        let relative = ImageRef {
            path: "sight.jpg".into(),
            ..image.clone()
        };
        assert!(relative.check().unwrap_err().0.contains("not absolute"));
        let empty = ImageRef { width: 0, ..image };
        assert!(empty.check().unwrap_err().0.contains("no pixels"));
    }

    #[test]
    fn a_joint_state_has_ros_lists_and_what_was_commanded() {
        let joints = JointState {
            name: vec!["yaw".into(), "pitch".into()],
            position: vec![0.125, -0.5],
            velocity: vec![0.0, 0.25],
            effort: vec![],
            commanded: vec![0.15, -0.5],
        };
        let text = "{\"name\":[\"yaw\",\"pitch\"],\"position\":[0.125,-0.5],\
                    \"velocity\":[0.0,0.25],\"effort\":[],\"commanded\":[0.15,-0.5]}";
        round_trip(&joints, text);
        needs::<JointState>(text, "commanded");
        assert_eq!(joints.check(), Ok(()));
        // As in ROS: a list is as long as the names, or empty.
        let short = JointState {
            velocity: vec![0.0],
            ..joints.clone()
        };
        let why = short.check().unwrap_err();
        assert!(
            why.0.contains("velocity has 1 values for 2 joints"),
            "{why}"
        );
        let none = JointState {
            name: vec![],
            position: vec![],
            velocity: vec![],
            effort: vec![],
            commanded: vec![],
        };
        assert_eq!(none.check(), Ok(()));
    }

    #[test]
    fn a_face_judged_for_speaking_says_its_state_and_score() {
        let speaking = Speaking {
            state: SpeakingState::Unknown,
            score: 0.0,
        };
        assert_eq!(
            serde_json::to_string(&speaking).unwrap(),
            "{\"state\":\"unknown\",\"score\":0.0}"
        );
        let read: Face = serde_json::from_str(
            "{\"id\":\"f1\",\"xmin\":0.1,\"ymin\":0.1,\"xmax\":0.2,\"ymax\":0.2,\"confidence\":0.9,\
             \"speaking\":{\"state\":\"yes\",\"score\":0.05}}",
        )
        .unwrap();
        assert_eq!(
            read.speaking,
            Some(Speaking {
                state: SpeakingState::Yes,
                score: 0.05
            })
        );
    }

    #[test]
    fn faces_are_a_list_of_boxes_in_fractions_of_the_picture() {
        let seen = Face {
            speaking: None,
            id: "f7".into(),
            xmin: 0.25,
            ymin: 0.125,
            xmax: 0.5,
            ymax: 0.625,
            confidence: 0.875,
            yaw: Some(-0.25),
            facing: Some(true),
        };
        let unjudged = Face {
            id: "f8".into(),
            yaw: None,
            facing: None,
            ..seen.clone()
        };
        let faces = Faces {
            faces: vec![seen.clone(), unjudged],
        };
        // What was not estimated is left out, not written as nothing.
        let text = "{\"faces\":[{\"id\":\"f7\",\"xmin\":0.25,\"ymin\":0.125,\"xmax\":0.5,\
                    \"ymax\":0.625,\"confidence\":0.875,\"yaw\":-0.25,\"facing\":true},\
                    {\"id\":\"f8\",\"xmin\":0.25,\"ymin\":0.125,\"xmax\":0.5,\"ymax\":0.625,\
                    \"confidence\":0.875}]}";
        round_trip(&faces, text);
        round_trip(&Faces { faces: vec![] }, "{\"faces\":[]}");
        needs::<Faces>(text, "faces");
        assert_eq!(faces.check(), Ok(()));
        // A field a later version adds to one face is passed over as well.
        let later = "{\"faces\":[{\"id\":\"f7\",\"xmin\":0.25,\"ymin\":0.125,\"xmax\":0.5,\
                     \"ymax\":0.625,\"confidence\":0.875,\"age\":31}]}";
        assert_eq!(
            serde_json::from_str::<Faces>(later).unwrap().faces[0].yaw,
            None
        );
        for wrong in [
            Face {
                xmax: 1.5,
                ..seen.clone()
            },
            Face {
                xmin: 0.75,
                ..seen.clone()
            },
            Face {
                ymin: -0.1,
                ..seen.clone()
            },
            Face {
                confidence: 1.25,
                ..seen.clone()
            },
            Face {
                xmin: f32::NAN,
                ..seen.clone()
            },
        ] {
            let faces = Faces { faces: vec![wrong] };
            assert!(faces.check().unwrap_err().0.contains("face \"f7\""));
        }
    }

    #[test]
    fn speech_is_text_that_is_finished_or_still_coming() {
        let whole = Speech {
            text: "lamp, dim a little".into(),
            finished: true,
            locale: Some("en_GB".into()),
            request: Some(7),
        };
        let text = "{\"text\":\"lamp, dim a little\",\"finished\":true,\"locale\":\"en_GB\",\
                    \"request\":7}";
        round_trip(&whole, text);
        needs::<Speech>(text, "finished");
        let coming = Speech {
            text: "lamp, d".into(),
            finished: false,
            locale: None,
            request: None,
        };
        round_trip(&coming, "{\"text\":\"lamp, d\",\"finished\":false}");
    }

    #[test]
    fn the_voice_says_which_of_four_things_it_is_doing() {
        for (state, word) in [
            (Voice::Resting, "resting"),
            (Voice::Listening, "listening"),
            (Voice::Thinking, "thinking"),
            (Voice::Speaking, "speaking"),
        ] {
            round_trip(&VoiceState { state }, &format!("{{\"state\":\"{word}\"}}"));
        }
        needs::<VoiceState>("{\"state\":\"resting\"}", "state");
        // A state this version does not know is not guessed at.
        assert!(serde_json::from_str::<VoiceState>("{\"state\":\"dreaming\"}").is_err());
        assert!(serde_json::from_str::<VoiceState>("{\"state\":\"Resting\"}").is_err());
    }

    #[test]
    fn the_link_is_ready_or_says_why_it_is_not() {
        round_trip(&VoiceLink::ready(), "{\"state\":\"ready\"}");
        for (reason, word) in [
            (LinkFault::Timeout, "timeout"),
            (LinkFault::Authentication, "authentication"),
            (LinkFault::Limited, "limited"),
            (LinkFault::Server, "server"),
            (LinkFault::Transport, "transport"),
            (LinkFault::Protocol, "protocol"),
            (LinkFault::Local, "local"),
        ] {
            let link = VoiceLink::unavailable(reason);
            round_trip(
                &link,
                &format!("{{\"state\":\"unavailable\",\"reason\":\"{word}\"}}"),
            );
            assert_eq!(link.check(), Ok(()));
        }
        assert_eq!(VoiceLink::ready().check(), Ok(()));
        let unexplained = VoiceLink {
            state: Link::Unavailable,
            reason: None,
        };
        assert!(unexplained.check().unwrap_err().0.contains("says why"));
        let both = VoiceLink {
            state: Link::Ready,
            reason: Some(LinkFault::Timeout),
        };
        assert!(both.check().unwrap_err().0.contains("no reason"));
        assert!(
            serde_json::from_str::<VoiceLink>("{\"state\":\"unavailable\",\"reason\":\"moon\"}")
                .is_err()
        );
    }

    #[test]
    fn the_room_is_unknown_quiet_or_busy() {
        for (state, word) in [
            (Room::Unknown, "unknown"),
            (Room::Quiet, "quiet"),
            (Room::Busy, "busy"),
        ] {
            round_trip(&RoomState { state }, &format!("{{\"state\":\"{word}\"}}"));
        }
        assert!(serde_json::from_str::<RoomState>("{\"state\":\"loud\"}").is_err());
    }

    #[test]
    fn a_light_is_on_or_off_in_one_of_ten_colours_at_one_of_five_levels() {
        let light = Light::new(true, "warm", 3).unwrap();
        let text = "{\"on\":true,\"colour\":\"warm\",\"level\":3}";
        round_trip(&light, text);
        needs::<Light>(text, "colour");
        assert_eq!(Light::COLOURS.len(), 10);
        for colour in Light::COLOURS {
            for level in 1..=5 {
                assert!(Light::new(false, colour, level).is_ok(), "{colour} {level}");
            }
            assert!(Light::is_colour(colour));
        }
        for colour in ["violet", "Warm", "", "warm white", "#ffdcb4"] {
            let why = Light::new(true, colour, 3).unwrap_err();
            assert!(why.0.contains("is not a colour of the light"), "{why}");
            assert!(!Light::is_colour(colour));
        }
        for level in [0, 6, 255] {
            let why = Light::new(true, "red", level).unwrap_err();
            assert!(why.0.contains("is not from 1 to 5"), "{why}");
        }
        // Read from the bus it is whatever was sent: the check is asked for.
        let read: Light = serde_json::from_str("{\"on\":true,\"colour\":\"teal\",\"level\":9}")
            .expect("the shape of a light");
        assert!(read.check().is_err());
        assert!(
            serde_json::from_str::<Light>("{\"on\":true,\"colour\":\"red\",\"level\":300}")
                .is_err()
        );
    }

    #[test]
    fn a_diagnostic_is_a_level_a_name_a_message_and_a_few_facts() {
        let report = Diagnostic {
            level: Level::Warn,
            name: "body".into(),
            message: "yaw is hot".into(),
            values: vec![KeyValue {
                key: "yaw_celsius".into(),
                value: "61".into(),
            }],
        };
        let text = "{\"level\":\"warn\",\"name\":\"body\",\"message\":\"yaw is hot\",\
                    \"values\":[{\"key\":\"yaw_celsius\",\"value\":\"61\"}]}";
        round_trip(&report, text);
        needs::<Diagnostic>(text, "message");
        for (level, word) in [
            (Level::Ok, "ok"),
            (Level::Warn, "warn"),
            (Level::Error, "error"),
            (Level::Stale, "stale"),
        ] {
            let well = Diagnostic {
                level,
                name: "voice".into(),
                message: String::new(),
                values: vec![],
            };
            round_trip(
                &well,
                &format!(
                    "{{\"level\":\"{word}\",\"name\":\"voice\",\"message\":\"\",\"values\":[]}}"
                ),
            );
        }
        // Said the short way, it is the same report.
        assert_eq!(
            Diagnostic::new(Level::Warn, "body", "yaw is hot").with("yaw_celsius", 61),
            report
        );
        assert_eq!(
            Diagnostic::ok("voice"),
            Diagnostic {
                level: Level::Ok,
                name: "voice".into(),
                message: String::new(),
                values: vec![],
            }
        );
        // ROS's own numbers are not this type's words.
        assert!(
            serde_json::from_str::<Diagnostic>(
                "{\"level\":1,\"name\":\"body\",\"message\":\"\",\"values\":[]}"
            )
            .is_err()
        );
    }

    #[test]
    fn the_lamps_health_is_its_parts_worst_first_and_the_worst_is_its_level() {
        let item = |name: &str, level, message: &str, age_ms| HealthItem {
            name: name.into(),
            level,
            message: message.into(),
            age_ms,
        };
        let health = Health {
            level: Level::Error,
            message: "voice: the link to the service is down".into(),
            items: vec![
                item(
                    "voice",
                    Level::Error,
                    "the link to the service is down",
                    300,
                ),
                item("body", Level::Warn, "yaw is hot", 420),
                item("eyes", Level::Stale, "", 7_300),
                item("memory", Level::Ok, "", 610),
            ],
            missing: vec!["board".into()],
        };
        let text = "{\"level\":\"error\",\"message\":\"voice: the link to the service is down\",\
                    \"items\":[{\"name\":\"voice\",\"level\":\"error\",\
                    \"message\":\"the link to the service is down\",\"age_ms\":300},\
                    {\"name\":\"body\",\"level\":\"warn\",\"message\":\"yaw is hot\",\"age_ms\":420},\
                    {\"name\":\"eyes\",\"level\":\"stale\",\"message\":\"\",\"age_ms\":7300},\
                    {\"name\":\"memory\",\"level\":\"ok\",\"message\":\"\",\"age_ms\":610}],\
                    \"missing\":[\"board\"]}";
        round_trip(&health, text);
        needs::<Health>(text, "missing");
        assert_eq!(health.check(), Ok(()));
        // The order a reader is given: ok, then quiet, then warned, then
        // wrong.
        let ranks = [Level::Ok, Level::Stale, Level::Warn, Level::Error].map(Level::rank);
        assert!(ranks.windows(2).all(|pair| pair[0] < pair[1]));
        // A level that is not the worst, and items out of order.
        let mut milder = health.clone();
        milder.level = Level::Warn;
        assert!(milder.check().unwrap_err().0.contains("worst"));
        let mut shuffled = health.clone();
        shuffled.items.swap(0, 3);
        assert!(shuffled.check().unwrap_err().0.contains("worst first"));
        // All well, and a part that never reported makes it stale.
        let mut well = Health {
            level: Level::Ok,
            message: String::new(),
            items: vec![item("voice", Level::Ok, "", 100)],
            missing: vec![],
        };
        assert_eq!(well.check(), Ok(()));
        well.missing.push("eyes".into());
        assert!(well.check().is_err());
        well.level = Level::Stale;
        assert_eq!(well.check(), Ok(()));
        let declared = Health::declaration();
        assert_eq!(declared.period(), Some(Duration::from_secs(5)));
        assert_eq!(declared.lifespan(), Some(Duration::from_secs(15)));
    }

    #[test]
    fn the_board_says_numbers_and_leaves_out_what_it_cannot_measure() {
        let board = BoardState {
            temperature_c: [("cpu".to_string(), 52.5)].into(),
            cpu_load: 0.25,
            memory_used: 0.5,
            disk_free: 0.125,
            uptime_s: 86_400,
            wifi_dbm: Some(-58),
            clock_synced: Some(true),
        };
        let text = "{\"temperature_c\":{\"cpu\":52.5},\"cpu_load\":0.25,\"memory_used\":0.5,\
                    \"disk_free\":0.125,\"uptime_s\":86400,\"wifi_dbm\":-58,\"clock_synced\":true}";
        round_trip(&board, text);
        needs::<BoardState>(text, "disk_free");
        assert_eq!(board.check(), Ok(()));
        // A board on a wire, whose clock nobody could ask about.
        let wired = BoardState {
            wifi_dbm: None,
            clock_synced: None,
            ..board.clone()
        };
        round_trip(
            &wired,
            "{\"temperature_c\":{\"cpu\":52.5},\"cpu_load\":0.25,\"memory_used\":0.5,\
             \"disk_free\":0.125,\"uptime_s\":86400}",
        );
        let mut full = board.clone();
        full.memory_used = 1.2;
        assert!(full.check().unwrap_err().0.contains("memory_used"));
        let mut odd = board.clone();
        odd.temperature_c.insert("gpu".into(), f32::NAN);
        assert!(odd.check().unwrap_err().0.contains("gpu"));
        let mut loaded = board;
        loaded.cpu_load = 3.5;
        assert_eq!(loaded.check(), Ok(()));
    }

    #[test]
    fn the_name_heard_is_a_score_and_how_long_ago_it_began() {
        let heard = NameHeard {
            score: 0.75,
            ago_ms: 640,
        };
        let text = "{\"score\":0.75,\"ago_ms\":640}";
        round_trip(&heard, text);
        needs::<NameHeard>(text, "ago_ms");
        assert_eq!(heard.check(), Ok(()));
        let odd = NameHeard {
            score: 1.5,
            ..heard
        };
        assert!(odd.check().unwrap_err().0.contains("score"));
        let declared = NameHeard::declaration();
        assert_eq!((declared.kind, declared.period()), (Kind::Event, None));
        // Its first sign is the same message on a topic of its own, not one
        // of the original seventeen.
        assert_eq!(declaration_of(NameHeard::RISING_TOPIC), Some(declared));
        assert!(
            first_topics()
                .iter()
                .all(|(topic, _)| *topic != NameHeard::RISING_TOPIC)
        );
    }

    #[test]
    fn a_decision_says_what_was_chosen_how_likely_by_whom_and_from_which_answers() {
        let decided = Decision {
            about: "attention".into(),
            choice: "look_up".into(),
            probability: 0.655,
            answers: BTreeMap::from([("others_business".into(), 0.33), ("calls_lamp".into(), 0.1)]),
            by: "typesafe/jev-1.13-20260917".into(),
            took_ms: 612,
            shadow: true,
            request: Some(7),
            task: None,
            arrival: None,
            ask: None,
        };
        // The answers are written by the question's name, in the order of
        // the names.
        let text = "{\"about\":\"attention\",\"choice\":\"look_up\",\"probability\":0.655,\
                    \"answers\":{\"calls_lamp\":0.1,\"others_business\":0.33},\
                    \"by\":\"typesafe/jev-1.13-20260917\",\"took_ms\":612,\"shadow\":true,\
                    \"request\":7}";
        round_trip(&decided, text);
        for field in [
            "about",
            "choice",
            "probability",
            "answers",
            "by",
            "took_ms",
            "shadow",
        ] {
            needs::<Decision>(text, field);
        }
        assert_eq!(decided.check(), Ok(()));
        // No model answered: a rule decided, from no answers, and nothing
        // says which request it was about.
        let ruled = Decision {
            choice: "stay_quiet".into(),
            probability: 0.26,
            answers: BTreeMap::new(),
            by: "rule".into(),
            took_ms: 3_000,
            request: None,
            ..decided.clone()
        };
        round_trip(
            &ruled,
            "{\"about\":\"attention\",\"choice\":\"stay_quiet\",\"probability\":0.26,\
             \"answers\":{},\"by\":\"rule\",\"took_ms\":3000,\"shadow\":true}",
        );
        assert_eq!(ruled.check(), Ok(()));
        // Both ends of a probability are one.
        for probability in [0.0, 1.0] {
            let edge = Decision {
                probability,
                answers: BTreeMap::from([("calls_lamp".into(), probability)]),
                ..decided.clone()
            };
            assert_eq!(edge.check(), Ok(()));
        }
        for probability in [1.5, -0.01, f32::NAN] {
            let wrong = Decision {
                probability,
                ..decided.clone()
            };
            let why = wrong.check().unwrap_err();
            assert!(why.0.contains("a decision's probability is"), "{why}");
            let wrong = Decision {
                answers: BTreeMap::from([("calls_lamp".into(), probability)]),
                ..decided.clone()
            };
            let why = wrong.check().unwrap_err();
            assert!(why.0.contains("the answer to calls_lamp is"), "{why}");
        }
        // Read from the bus it is whatever was sent: the check is asked for.
        assert!(
            serde_json::from_str::<Decision>(
                "{\"about\":\"attention\",\"choice\":\"answer\",\"probability\":\"high\",\
                 \"answers\":{},\"by\":\"rule\",\"took_ms\":0,\"shadow\":true}"
            )
            .is_err()
        );
    }

    #[test]
    fn the_hand_is_kept_apart_from_the_first_topics_and_still_recognised() {
        assert!(!first_topics().iter().any(|(name, _)| *name == Hand::TOPIC));
        let declared = declaration_of(Hand::TOPIC).unwrap();
        assert_eq!(
            (declared.type_name.as_str(), declared.kind),
            (Hand::TYPE, Kind::Latest)
        );
    }

    #[test]
    fn a_body_gesture_says_its_name_and_what_became_of_it() {
        for (state, word) in [
            (GestureState::Began, "began"),
            (GestureState::Over, "over"),
            (GestureState::Dropped, "dropped"),
        ] {
            round_trip(
                &BodyGesture {
                    name: "nod".into(),
                    state,
                },
                &format!("{{\"name\":\"nod\",\"state\":\"{word}\"}}"),
            );
        }
        assert!(
            !first_topics()
                .iter()
                .any(|(name, _)| *name == BodyGesture::TOPIC)
        );
        let declared = declaration_of(BodyGesture::TOPIC).unwrap();
        assert_eq!(
            (declared.type_name.as_str(), declared.kind),
            (BodyGesture::TYPE, Kind::Event)
        );
    }

    #[test]
    fn the_voice_mute_says_why_only_while_muted() {
        let asleep = VoiceMute {
            muted: true,
            reason: Some(MuteReason::Sleep),
        };
        round_trip(&asleep, "{\"muted\":true,\"reason\":\"sleep\"}");
        assert!(asleep.check().is_ok() && asleep.asleep());
        // The mute as it always was writes no reason.
        let muted = VoiceMute {
            muted: true,
            reason: None,
        };
        round_trip(&muted, "{\"muted\":true}");
        assert!(muted.check().is_ok() && !muted.asleep());
        needs::<VoiceMute>("{\"muted\":true}", "muted");
        let wrong = VoiceMute {
            muted: false,
            reason: Some(MuteReason::Sleep),
        };
        assert!(wrong.check().is_err() && !wrong.asleep());
        assert!(serde_json::from_str::<VoiceMute>("{\"muted\":true,\"reason\":\"nap\"}").is_err());
        assert!(
            !first_topics()
                .iter()
                .any(|(name, _)| *name == VoiceMute::TOPIC)
        );
        let declared = declaration_of(VoiceMute::TOPIC).unwrap();
        assert_eq!(
            (
                declared.type_name.as_str(),
                declared.kind,
                declared.period_ms
            ),
            (VoiceMute::TYPE, Kind::Latest, Some(1_000))
        );
    }

    #[test]
    fn the_pause_is_kept_apart_from_the_first_topics_and_still_recognised() {
        assert!(!first_topics().iter().any(|(name, _)| *name == Pause::TOPIC));
        let declared = declaration_of(Pause::TOPIC).unwrap();
        assert_eq!(
            (
                declared.type_name.as_str(),
                declared.kind,
                declared.period_ms
            ),
            (Pause::TYPE, Kind::Latest, None)
        );
    }

    #[test]
    fn rules_are_booleans_and_a_count_kept_apart_from_the_first_topics() {
        round_trip(
            &Rules {
                person: "p-be9688998f085098".into(),
                quiet: false,
                quiet_mornings: true,
                rules: 2,
                hello: Unprompted {
                    not_before_minute: Some(540),
                    ..Unprompted::default()
                },
                news: Unprompted {
                    not_while_talking: true,
                    ..Unprompted::default()
                },
            },
            "{\"person\":\"p-be9688998f085098\",\"quiet\":false,\"quiet_mornings\":true,\"rules\":2,\
             \"hello\":{\"never\":false,\"not_before_minute\":540,\"not_from_minute\":null,\"not_while_talking\":false},\
             \"news\":{\"never\":false,\"not_before_minute\":null,\"not_from_minute\":null,\"not_while_talking\":true}}",
        );
        // A message of before #813's typed rules still reads: nothing held.
        let old: Rules = serde_json::from_str(
            "{\"person\":\"everyone\",\"quiet\":true,\"quiet_mornings\":true,\"rules\":1}",
        )
        .unwrap();
        assert_eq!((old.hello, old.news), Default::default());
        assert!(!first_topics().iter().any(|(name, _)| *name == Rules::TOPIC));
        let declared = declaration_of(Rules::TOPIC).unwrap();
        assert_eq!(
            (declared.type_name.as_str(), declared.kind),
            (Rules::TYPE, Kind::Latest)
        );
    }

    #[test]
    fn every_first_topic_is_declared_as_the_table_of_first_topics_has_it() {
        // docs/bus.md, "The first topics": a stream is an event, a latched
        // topic keeps its latest, and "how often" is the period promised,
        // the slower end where the table gives two. The policy's topic,
        // the lamp's health and the board were added since and are in the
        // table of docs/msgs.md.
        let table: [(&str, &str, Kind, Option<u64>); 17] = [
            (
                "board/state",
                "lamp_msgs/BoardState",
                Kind::Latest,
                Some(5_000),
            ),
            (
                "body/joint_states",
                "lamp_msgs/JointState",
                Kind::Latest,
                Some(100),
            ),
            (
                "camera/image",
                "lamp_msgs/ImageRef",
                Kind::Latest,
                Some(500),
            ),
            (
                "diagnostics",
                "lamp_msgs/Diagnostic",
                Kind::Event,
                Some(1_000),
            ),
            (
                "humans/faces/tracked",
                "lamp_msgs/Faces",
                Kind::Latest,
                None,
            ),
            ("lamp/health", "lamp_msgs/Health", Kind::Latest, Some(5_000)),
            (
                "light/command",
                "lamp_msgs/Light",
                Kind::Latest,
                Some(1_000),
            ),
            ("light/state", "lamp_msgs/Light", Kind::Latest, Some(1_000)),
            ("mic1/audio", "lamp_msgs/Audio", Kind::Event, Some(20)),
            ("mic2/audio", "lamp_msgs/Audio", Kind::Event, Some(20)),
            ("name/heard", "lamp_msgs/NameHeard", Kind::Event, None),
            ("policy/decisions", "lamp_msgs/Decision", Kind::Event, None),
            ("room/state", "lamp_msgs/RoomState", Kind::Latest, None),
            ("voice/heard", "lamp_msgs/Speech", Kind::Event, None),
            (
                "voice/link",
                "lamp_msgs/VoiceLink",
                Kind::Latest,
                Some(1_000),
            ),
            ("voice/said", "lamp_msgs/Speech", Kind::Event, None),
            (
                "voice/state",
                "lamp_msgs/VoiceState",
                Kind::Latest,
                Some(1_000),
            ),
        ];
        let declared = first_topics();
        assert_eq!(declared.len(), table.len());
        for ((topic, declaration), (name, type_name, kind, period_ms)) in declared.iter().zip(table)
        {
            assert_eq!(*topic, name);
            assert_eq!(declaration.type_name, type_name, "{topic}");
            assert_eq!(declaration.version, 1, "{topic}");
            assert_eq!(declaration.kind, kind, "{topic}");
            assert_eq!(declaration.period_ms, period_ms, "{topic}");
            // No lifespan is in the first table, and none is made up. The
            // two kept reports of how things are go stale by themselves.
            let lifespan = ["board/state", "lamp/health"]
                .contains(topic)
                .then_some(15_000);
            assert_eq!(declaration.lifespan_ms, lifespan, "{topic}");
            assert_eq!(declaration_of(topic).as_ref(), Some(declaration));
        }
        assert_eq!(declaration_of("servos"), None);
        // Each type says the same of itself.
        for (type_name, version, declaration) in [
            (Audio::TYPE, Audio::VERSION, Audio::declaration()),
            (ImageRef::TYPE, ImageRef::VERSION, ImageRef::declaration()),
            (
                JointState::TYPE,
                JointState::VERSION,
                JointState::declaration(),
            ),
            (Faces::TYPE, Faces::VERSION, Faces::declaration()),
            (Speech::TYPE, Speech::VERSION, Speech::declaration()),
            (
                VoiceState::TYPE,
                VoiceState::VERSION,
                VoiceState::declaration(),
            ),
            (
                VoiceLink::TYPE,
                VoiceLink::VERSION,
                VoiceLink::declaration(),
            ),
            (
                RoomState::TYPE,
                RoomState::VERSION,
                RoomState::declaration(),
            ),
            (Light::TYPE, Light::VERSION, Light::declaration()),
            (
                Diagnostic::TYPE,
                Diagnostic::VERSION,
                Diagnostic::declaration(),
            ),
            (Health::TYPE, Health::VERSION, Health::declaration()),
            (
                BoardState::TYPE,
                BoardState::VERSION,
                BoardState::declaration(),
            ),
            (
                NameHeard::TYPE,
                NameHeard::VERSION,
                NameHeard::declaration(),
            ),
            (Decision::TYPE, Decision::VERSION, Decision::declaration()),
        ] {
            assert_eq!(declaration.type_name, type_name);
            assert_eq!(declaration.version, version);
            assert!(type_name.starts_with("lamp_msgs/"));
        }
    }
}
