//! The protocol's messages, v1 (docs/control.md, "The wire protocol").
//!
//! One JSON object per WebSocket text frame (or per line on the local
//! socket, later). A request is `{"id": N, "op": "...", ...}`; its reply is
//! `{"re": N, "ok": true, ...}` or `{"re": N, "ok": false, "error": {"code":
//! "...", "why": "..."}}`; an event is `{"ev": "...", ...}`. A field this
//! version does not know refuses the request (`bad_request`), so a typo is
//! never a step silently not taken.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// The protocol's version, as the path (`/v1`) and `hello` say it.
pub const VERSION: u32 = 1;

/// The longest `say`, in UTF-8 bytes: the voice's own (`lamp_voice::say`'s
/// `MOST_WORDS_BYTES`), so a say the voice would refuse is refused here, in
/// words, never answered `{"to":"voice"}` and then dropped.
pub const SAY_MAX_BYTES: usize = 900;
/// How many phrases one client may listen for (`phrase.heard`).
pub const PHRASES_MAX: usize = 16;
/// The longest phrase, in characters.
pub const PHRASE_MAX_CHARS: usize = 64;
/// A phrase holds at least this many letters, in one word or more, so a
/// program can't register single short words ("a", "yes") or short pairs
/// ("is a", "it is") and probe the room's speech by them (Fourteen's P2 and
/// harness-reviewer's P3 on #3868). It is friction, not a guard: longer
/// common phrases pass, and `hear` is kept narrow by the grant, the cap of
/// [`PHRASES_MAX`], the rate on changing them, and that only the registered
/// phrase leaves the lamp (Fourteen's P3 on #3901).
pub const PHRASE_MIN_LETTERS: usize = 8;

/// A phrase's or a heard line's words, as both the floor and the match see
/// them: split on anything but letters, digits and the apostrophe (so
/// "don't" is one word in both places, the Reviewer-lead's P3 on #3868),
/// lowercased.
pub fn phrase_words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}
/// The longest a light request holds, in ms: as the ring's own light
/// requests ([`lamp_msgs::LightRequest`]'s claims, ≤ 30 s).
pub const HOLD_MAX_MS: u32 = 30_000;
/// The longest one lease take or renewal asks for, in ms: ten minutes. A
/// lease renewed in time is held as long as its holder renews (the owner's
/// "no limits for developers", 30 Oct).
pub const LEASE_MAX_MS: u32 = 600_000;
/// A move's step, in ms: the body's own (`lamp_body::moves::check`).
pub const STEP_MS: std::ops::RangeInclusive<u32> = 40..=1_000;
/// A move's fewest frames: a start and an end. Its most are the body's own
/// bound, held by `lamp_body::moves::check` (at most 10 s, so at most 251
/// frames at the shortest step); the frame's 64 KiB bounds the list read.
pub const FRAMES_MIN: usize = 2;
/// The longest name a client gives itself at pairing, in characters.
pub const CLIENT_NAME_MAX_CHARS: usize = 64;
/// A pushed skill package's largest size, in bytes.
pub const SKILL_PACKAGE_MAX: usize = 1 << 20;
/// One `skill.push` part's largest `data`, in base64 characters (36 KiB of
/// the package), so a frame stays under 64 KiB.
pub const SKILL_PART_MAX_CHARS: usize = 48 * 1024;
/// The most parts one push is sent in.
pub const SKILL_PARTS_MAX: u32 = 32;
/// The longest skill name, as the skill format has it.
pub const SKILL_NAME_MAX_CHARS: usize = 64;

/// A part a lease holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Part {
    Body,
    Light,
    Voice,
}

/// What the owner granted a paired client, as the app shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Grant {
    /// Gestures, its own moves, looking, joints under a lease, feelings.
    Body,
    /// The light, and feelings.
    Light,
    /// `say` and `sound`; with `hear`, `turn.start`; `turn.end`.
    Speak,
    /// Presence: arrivals, leavings, facing, standing up (positions, never
    /// who).
    Look,
    /// Its own phrases heard, never the words around them, and the name;
    /// with `speak`, `turn.start`; `listen.set`, its own mute.
    Hear,
    /// Sound classes heard.
    Sound,
    /// Its own skills put on the lamp (`skill.push`, `skill.list`,
    /// `skill.remove`), which run on their own after it goes, never beyond
    /// its other grants (docs/control.md, "Skills pushed from a computer").
    Skills,
    /// The air sensor's readings (`air/state`): particles, carbon dioxide,
    /// humidity and temperature at the sensor; `get` `air` and `air.changed`.
    Air,
    /// The lamp's own computer (`board/state`): processor load, memory,
    /// disk and its temperatures; `get` `device`.
    Device,
    /// The camera's pictures: `camera.snapshot` and `camera.stream`, the
    /// eyes' own newest frame, never while the lamp is private.
    Camera,
    /// The microphone's sound as it is heard, never while the lamp is
    /// private.
    Microphone,
}

impl Grant {
    /// What the pairing screen says the owner is granting, word for word:
    /// the owner's admin socket gives these with the code, and the app
    /// shows them. `skills` says that what it puts on keeps running after
    /// the computer goes (docs/control.md, "Skills pushed from a computer").
    pub fn shown(self) -> &'static str {
        match self {
            Self::Body => "can move the lamp's body",
            Self::Light => "can change the light",
            Self::Speak => "can speak and play sounds",
            Self::Look => "can tell when someone is there, never who",
            Self::Hear => "can hear its own phrases, never the words around them",
            Self::Sound => "can hear kinds of sound, never words",
            Self::Skills => "can put skills on the lamp that run on their own",
            Self::Air => {
                "can read the air sensor: particles, carbon dioxide and humidity, which can show when someone is in the room"
            }
            Self::Device => "can read the lamp's processor, memory, disk and temperature",
            Self::Camera => "can see what the lamp's camera sees, never while it is private",
            Self::Microphone => {
                "can hear what the lamp's microphone hears, never while it is private"
            }
        }
    }
}

/// An event a client may subscribe to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum EventKind {
    #[serde(rename = "person.arrived")]
    PersonArrived,
    #[serde(rename = "person.left")]
    PersonLeft,
    #[serde(rename = "person.stood_up")]
    PersonStoodUp,
    #[serde(rename = "person.facing")]
    PersonFacing,
    /// A hand in view, where it is and whether it is open; or gone.
    #[serde(rename = "hand")]
    Hand,
    #[serde(rename = "phrase.heard")]
    PhraseHeard,
    #[serde(rename = "name.heard")]
    NameHeard,
    #[serde(rename = "sound.heard")]
    SoundHeard,
    #[serde(rename = "touch")]
    Touch,
    #[serde(rename = "button")]
    Button,
    #[serde(rename = "time")]
    Time,
    #[serde(rename = "lamp.changed")]
    LampChanged,
    /// The air sensor's reading, when a value moved past its step, at most
    /// once every 5 s (`air/state`).
    #[serde(rename = "air.changed")]
    AirChanged,
    /// The speaker's volume changed, whoever changed it: `level`, `muted`.
    #[serde(rename = "volume.changed")]
    VolumeChanged,
}

impl EventKind {
    /// The grant a subscription to it needs, if any.
    pub fn needs(self) -> Option<Grant> {
        match self {
            Self::PersonArrived
            | Self::PersonLeft
            | Self::PersonStoodUp
            | Self::PersonFacing
            | Self::Hand => Some(Grant::Look),
            Self::PhraseHeard | Self::NameHeard => Some(Grant::Hear),
            Self::SoundHeard => Some(Grant::Sound),
            Self::AirChanged => Some(Grant::Air),
            Self::VolumeChanged => Some(Grant::Speak),
            Self::Touch | Self::Button | Self::Time | Self::LampChanged => None,
        }
    }
}

/// A piece of state a client may read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateKind {
    /// How many are in view, how near, whether facing: never who.
    Presence,
    /// The person's own light and what the ring shows.
    Light,
    /// The inner state's energy, warmth, awake and asleep.
    Mood,
    /// Talking, paused, private, asleep, quiet hours, the lease holder.
    Lamp,
    /// The air sensor's newest reading, or `known: false` (no sensor, or
    /// none in 15 s).
    Air,
    /// The lamp's own computer: processor load, memory, disk, temperatures.
    Device,
    /// The speaker's volume, 0 to 100, and whether it is muted.
    Volume,
    /// What the body may do, joint by joint, and where each joint is now
    /// (`crate::body_info`), or `known: false` (none in 15 s).
    Body,
}

impl StateKind {
    /// The grant reading it needs, if any.
    pub fn needs(self) -> Option<Grant> {
        match self {
            Self::Presence => Some(Grant::Look),
            Self::Air => Some(Grant::Air),
            Self::Device => Some(Grant::Device),
            Self::Volume => Some(Grant::Speak),
            // The body's bounds and where it is: as `body.state`, with the
            // body grant.
            Self::Body => Some(Grant::Body),
            Self::Light | Self::Mood | Self::Lamp => None,
        }
    }
}

/// Where `look` turns.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LookAt {
    /// The face in view, the hand in view (followed), the voice that
    /// called, or straight ahead.
    Target(LookTarget),
    /// Degrees from straight ahead: yaw right positive, pitch up positive.
    Angles(Angles),
}

/// `look`'s angles, in degrees. A field it doesn't know refuses it, as
/// everywhere else (the Reviewer-lead's P3 on #3715: untagged, `roll` was
/// dropped without a word).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Angles {
    pub yaw: f32,
    pub pitch: f32,
}

/// The sounds a computer may ask for by the lamp's own name: the voice's
/// built-in hum. Any other name is one of the computer's own clips
/// (`sound.upload`, `crate::sounds`).
pub const SOUNDS: [&str; 1] = ["hum"];
/// The most parts one clip is sent in: a 30 s clip, about 1.4 MB, is 40
/// parts of [`SKILL_PART_MAX_CHARS`].
pub const SOUND_PARTS_MAX: u32 = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LookTarget {
    Face,
    /// The hand in view, followed by the body while the lease holds.
    Hand,
    Name,
    Ahead,
}

/// `joints`' goals, in degrees from each joint's home; a joint not named
/// stays where it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Joints {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitch: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roll: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yaw: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shoulder: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elbow: Option<f32>,
}

impl Joints {
    fn values(&self) -> [Option<f32>; 5] {
        [self.pitch, self.roll, self.yaw, self.shoulder, self.elbow]
    }
}

/// How a light moves: the ring's own three ([`lamp_msgs::PatternKind`]'s
/// breathe, candle and pulse), each at the ring's default period.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pattern {
    Breathe,
    Candle,
    Pulse,
}

fn half() -> f32 {
    0.5
}

/// A request's operation and its fields.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", deny_unknown_fields)]
pub enum Op {
    /// Once per computer: its Ed25519 public key (base64) and the proof of
    /// the owner's one-time code over it.
    #[serde(rename = "pair")]
    Pair {
        name: String,
        key: String,
        proof: String,
    },
    /// Every connection: the client's id and its signature over the lamp's
    /// nonce.
    #[serde(rename = "hello")]
    Hello { client: String, sig: String },
    #[serde(rename = "subscribe")]
    Subscribe {
        events: Vec<EventKind>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        phrases: Vec<String>,
    },
    #[serde(rename = "unsubscribe")]
    Unsubscribe { events: Vec<EventKind> },
    #[serde(rename = "get")]
    Get { state: Vec<StateKind> },
    #[serde(rename = "lease.take")]
    LeaseTake {
        parts: Vec<Part>,
        for_ms: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        why: Option<String>,
    },
    #[serde(rename = "lease.renew")]
    LeaseRenew { lease: String, for_ms: u32 },
    #[serde(rename = "lease.release")]
    LeaseRelease { lease: String },
    /// One of the lamp's own feelings (the `feelings` skill's), 0 to 1.
    #[serde(rename = "express")]
    Express {
        feeling: String,
        #[serde(default = "half")]
        strength: f32,
    },
    /// One of the body's built-in gestures, by name.
    #[serde(rename = "gesture")]
    Gesture { name: String },
    #[serde(rename = "look")]
    Look { at: LookAt },
    /// A move of its own: five joints a frame (pitch, roll, yaw, shoulder,
    /// elbow, in the body's counts from home), starting and ending at home.
    #[serde(rename = "move")]
    Move { step_ms: u32, frames: Vec<[i32; 5]> },
    /// A recorded move of the client's package, by name.
    #[serde(rename = "play")]
    Play { name: String },
    #[serde(rename = "joints")]
    Joints {
        lease: String,
        targets: Joints,
        within_ms: u32,
    },
    /// A light for a while: one of the ring's named colours at a level,
    /// or the computer's own colour (`rgb`) or each pixel's (`pixels`), at
    /// a `brightness` (docs/control.md, "The ring, pixel by pixel").
    #[serde(rename = "light")]
    Light {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        colour: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        level: Option<u8>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rgb: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pixels: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        brightness: Option<u8>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pattern: Option<Pattern>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fade_ms: Option<u32>,
        for_ms: u32,
    },
    /// A line in the lamp's own voice: spoken exactly as sent, straight
    /// through the speech service, unless `rephrase` asks the voice's model
    /// to say it in its own words, as before (docs/control.md, "say").
    #[serde(rename = "say")]
    Say {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rephrase: Option<bool>,
    },
    /// The lamp's own hum, or one of the computer's own clips by name.
    #[serde(rename = "sound")]
    Sound { name: String },
    /// One part of a clip of the computer's own, a WAV sent in order from
    /// 0 (`crate::sounds`); the last part's answer says it's kept.
    #[serde(rename = "sound.upload")]
    SoundUpload {
        name: String,
        sha256: String,
        size: usize,
        part: u32,
        parts: u32,
        data: String,
    },
    /// The computer's own clips.
    #[serde(rename = "sound.list")]
    SoundList {},
    /// One of the computer's own clips, deleted.
    #[serde(rename = "sound.remove")]
    SoundRemove { name: String },
    /// Opens a conversation turn as a tap on the head does: the person's
    /// next line is answered as spoken to the lamp. With `prompt`, the lamp
    /// says it first (docs/control.md, "Turns and listening").
    #[serde(rename = "turn.start")]
    TurnStart {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt: Option<String>,
    },
    /// Ends the turn this computer started, and the conversation grown from
    /// it; never another's.
    #[serde(rename = "turn.end")]
    TurnEnd {},
    /// `false`: this computer's mute, held until it says `true`, its last
    /// connection closes, it is revoked or a hand on the head lifts it.
    #[serde(rename = "listen.set")]
    ListenSet { on: bool },
    /// Ends the client's own actions now. Needs no lease.
    #[serde(rename = "stop")]
    Stop {},
    /// One part of a skill package, `lamp-skills pack`'s shape, sent in
    /// order from 0; the last part's answer comes once it is installed.
    #[serde(rename = "skill.push")]
    SkillPush {
        name: String,
        sha256: String,
        size: usize,
        part: u32,
        parts: u32,
        data: String,
    },
    /// The speaker's volume, the person's own: exactly one of a level
    /// (`to`, 0 to 100), a step, or a mute or an unmute.
    #[serde(rename = "volume")]
    Volume {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<u8>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        step: Option<VolumeStep>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mute: Option<bool>,
    },
    /// The client's own pushed skills.
    #[serde(rename = "skill.list")]
    SkillList {},
    /// One of the client's own pushed skills, taken off.
    #[serde(rename = "skill.remove")]
    SkillRemove { name: String },
    /// The camera's newest picture, once: a JPEG, as the eyes keep it.
    #[serde(rename = "camera.snapshot")]
    CameraSnapshot {},
    /// The camera's pictures as `camera.frame` events, `fps` a second at
    /// most, until `on: false`, `stop`, the client goes or the lamp turns
    /// private (`camera.ended`, with why).
    #[serde(rename = "camera.stream")]
    CameraStream {
        on: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fps: Option<u8>,
        /// The fast stream's size, `640x480` (the default) or `1280x720`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size: Option<String>,
    },
}

impl Op {
    /// The grants it needs (all of them).
    pub fn needs(&self) -> Vec<Grant> {
        let mut needs: Vec<Grant> = match self {
            Self::Pair { .. }
            | Self::Hello { .. }
            | Self::Unsubscribe { .. }
            | Self::LeaseRelease { .. }
            | Self::LeaseRenew { .. }
            | Self::Stop {} => Vec::new(),
            Self::Subscribe { events, .. } => events.iter().filter_map(|e| e.needs()).collect(),
            Self::Get { state } => state.iter().filter_map(|s| s.needs()).collect(),
            Self::LeaseTake { parts, .. } => parts
                .iter()
                .map(|part| match part {
                    Part::Body => Grant::Body,
                    Part::Light => Grant::Light,
                    Part::Voice => Grant::Speak,
                })
                .collect(),
            Self::Express { .. } => vec![Grant::Body, Grant::Light],
            Self::Gesture { .. } | Self::Move { .. } | Self::Play { .. } | Self::Joints { .. } => {
                vec![Grant::Body]
            }
            Self::Look { at } => match at {
                LookAt::Target(LookTarget::Face | LookTarget::Hand) => {
                    vec![Grant::Body, Grant::Look]
                }
                _ => vec![Grant::Body],
            },
            Self::Light { .. } => vec![Grant::Light],
            Self::Say { .. }
            | Self::Sound { .. }
            | Self::SoundUpload { .. }
            | Self::SoundList {}
            | Self::SoundRemove { .. }
            | Self::Volume { .. } => vec![Grant::Speak],
            Self::TurnEnd {} => vec![Grant::Speak],
            // A turn speaks and hears what is said to the lamp.
            Self::TurnStart { .. } => vec![Grant::Speak, Grant::Hear],
            Self::ListenSet { .. } => vec![Grant::Hear],
            Self::SkillPush { .. } | Self::SkillList {} | Self::SkillRemove { .. } => {
                vec![Grant::Skills]
            }
            Self::CameraSnapshot {} | Self::CameraStream { .. } => vec![Grant::Camera],
        };
        needs.sort();
        needs.dedup();
        needs
    }

    /// The part a lease must hold for it, if any: the body's own moves,
    /// looks and joints (docs/control.md, "Who wins"). A feeling or a
    /// built-in gesture is one short beat and needs none.
    pub fn needs_lease(&self) -> Option<Part> {
        match self {
            Self::Move { .. } | Self::Play { .. } | Self::Look { .. } | Self::Joints { .. } => {
                Some(Part::Body)
            }
            _ => None,
        }
    }

    /// Whether the client must have said `hello` (or be pairing) first:
    /// every op but `pair` and `hello`.
    pub fn needs_hello(&self) -> bool {
        !matches!(self, Self::Pair { .. } | Self::Hello { .. })
    }

    /// The op's own checks, beyond its fields' types: why it is refused.
    pub fn check(&self) -> Result<(), String> {
        match self {
            Self::Pair { name, key, proof } => {
                let chars = name.chars().count();
                if chars == 0 || chars > CLIENT_NAME_MAX_CHARS || name.chars().any(char::is_control)
                {
                    return Err(format!(
                        "name: 1 to {CLIENT_NAME_MAX_CHARS} characters, none a control"
                    ));
                }
                if !base64_of(key, 32) {
                    return Err("key: an Ed25519 public key, 32 bytes as base64".to_owned());
                }
                if !base64_of(proof, 32) {
                    return Err("proof: an HMAC-SHA256, 32 bytes as base64".to_owned());
                }
            }
            Self::Hello { client, sig } => {
                if !client_id(client) {
                    return Err("client: the id pairing gave, c_ and 1 to 32 hex".to_owned());
                }
                if !base64_of(sig, 64) {
                    return Err("sig: an Ed25519 signature, 64 bytes as base64".to_owned());
                }
            }
            Self::Subscribe { events, phrases } => {
                if events.is_empty() {
                    return Err("events: at least one".to_owned());
                }
                if phrases.len() > PHRASES_MAX {
                    return Err(format!("phrases: at most {PHRASES_MAX}"));
                }
                if !phrases.is_empty() && !events.contains(&EventKind::PhraseHeard) {
                    return Err("phrases: only with phrase.heard".to_owned());
                }
                if events.contains(&EventKind::PhraseHeard) && phrases.is_empty() {
                    return Err("phrase.heard: name the phrases to listen for".to_owned());
                }
                for phrase in phrases {
                    let chars = phrase.chars().count();
                    if !phrase.chars().any(char::is_alphanumeric)
                        || chars > PHRASE_MAX_CHARS
                        || phrase.chars().any(char::is_control)
                    {
                        return Err(format!(
                            "phrases: words of at most {PHRASE_MAX_CHARS} characters"
                        ));
                    }
                    let letters = phrase.chars().filter(|c| c.is_alphanumeric()).count();
                    if letters < PHRASE_MIN_LETTERS {
                        return Err(format!(
                            "phrases: {phrase:?} is too short: {PHRASE_MIN_LETTERS} letters at least"
                        ));
                    }
                }
            }
            Self::SkillPush {
                name,
                sha256,
                size,
                part,
                parts,
                data,
            } => {
                skill_name(name)?;
                if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err("sha256: the package's SHA-256, 64 hex".to_owned());
                }
                if *size == 0 || *size > SKILL_PACKAGE_MAX {
                    return Err(format!("size: 1 to {SKILL_PACKAGE_MAX} bytes"));
                }
                if *parts == 0 || *parts > SKILL_PARTS_MAX || part >= parts {
                    return Err(format!(
                        "parts: 1 to {SKILL_PARTS_MAX}, and part from 0 to parts - 1"
                    ));
                }
                if data.is_empty() || data.len() > SKILL_PART_MAX_CHARS {
                    return Err(format!(
                        "data: 1 to {SKILL_PART_MAX_CHARS} characters of base64"
                    ));
                }
            }
            Self::SkillRemove { name } => skill_name(name)?,
            Self::CameraStream { fps, size, .. } => {
                if fps.is_some_and(|fps| !(1..=crate::camera::FPS_MOST).contains(&fps)) {
                    return Err(format!("fps: 1 to {}", crate::camera::FPS_MOST));
                }
                if size
                    .as_deref()
                    .is_some_and(|size| crate::camera::size_of(size).is_none())
                {
                    return Err("size: 640x480 or 1280x720".to_owned());
                }
            }
            Self::Unsubscribe { events } => {
                if events.is_empty() {
                    return Err("events: at least one".to_owned());
                }
            }
            Self::Get { state } => {
                if state.is_empty() {
                    return Err("state: at least one".to_owned());
                }
            }
            Self::LeaseTake { parts, for_ms, why } => {
                if parts.is_empty() {
                    return Err("parts: at least one".to_owned());
                }
                held_for(*for_ms, LEASE_MAX_MS)?;
                if why.as_ref().is_some_and(|why| why.chars().count() > 200) {
                    return Err("why: at most 200 characters".to_owned());
                }
            }
            Self::LeaseRenew { lease, for_ms } => {
                lease_id(lease)?;
                held_for(*for_ms, LEASE_MAX_MS)?;
            }
            Self::LeaseRelease { lease } => lease_id(lease)?,
            Self::Express { feeling, strength } => {
                plain_name(feeling, "feeling")?;
                if !(0.0..=1.0).contains(strength) {
                    return Err("strength: from 0 to 1".to_owned());
                }
            }
            // Any gesture the body has, `idle` and `scanning` too: the body
            // answers a name it doesn't know (the owner's "no limits for
            // developers", 30 Oct).
            Self::Gesture { name } | Self::Play { name } => {
                plain_name(name, "name")?;
            }
            // The lamp's own short hum, or a clip of the computer's own by
            // its name; the recorded notices (no Wi-Fi) are never a
            // computer's to play, and no clip path is ever asked.
            Self::Sound { name } => {
                if !SOUNDS.contains(&name.as_str()) {
                    plain_name(name, "name")?;
                }
            }
            Self::SoundUpload {
                name,
                sha256,
                size,
                part,
                parts,
                data,
            } => {
                clip_name(name)?;
                if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err("sha256: the clip's SHA-256, 64 hex".to_owned());
                }
                if *size == 0 || *size > crate::sounds::CLIP_MOST_BYTES {
                    return Err(format!(
                        "size: 1 to {} bytes",
                        crate::sounds::CLIP_MOST_BYTES
                    ));
                }
                if *parts == 0 || *parts > SOUND_PARTS_MAX || part >= parts {
                    return Err(format!(
                        "parts: 1 to {SOUND_PARTS_MAX}, and part from 0 to parts - 1"
                    ));
                }
                if data.is_empty() || data.len() > SKILL_PART_MAX_CHARS {
                    return Err(format!(
                        "data: 1 to {SKILL_PART_MAX_CHARS} characters of base64"
                    ));
                }
            }
            Self::SoundRemove { name } => clip_name(name)?,
            // Any finite angles: the body clamps them to its reach and says
            // so in the done (`clamped`).
            Self::Look { at } => {
                if let LookAt::Angles(Angles { yaw, pitch }) = at
                    && !(yaw.is_finite() && pitch.is_finite())
                {
                    return Err("at: yaw and pitch, finite degrees".to_owned());
                }
            }
            Self::Move { step_ms, frames } => {
                if !STEP_MS.contains(step_ms) {
                    return Err(format!(
                        "step_ms: from {} to {}",
                        STEP_MS.start(),
                        STEP_MS.end()
                    ));
                }
                if frames.len() < FRAMES_MIN {
                    return Err(format!("frames: at least {FRAMES_MIN}"));
                }
                if frames.first() != Some(&[0; 5]) || frames.last() != Some(&[0; 5]) {
                    return Err("frames: start and end at home, all five 0".to_owned());
                }
                // Held to the body's own check here, so a move the body
                // would drop (too short, too far, too fast) is refused with
                // its words, never answered `{"to":"body"}` and then lost
                // (Ten's finding on #3997, 29 Oct).
                let frames = frames
                    .iter()
                    .map(|frame| frame.map(|counts| i16::try_from(counts).unwrap_or(i16::MAX)))
                    .collect();
                let file = lamp_body::moves::Move {
                    name: "control".to_owned(),
                    step_ms: u16::try_from(*step_ms).unwrap_or(u16::MAX),
                    frames,
                };
                lamp_body::moves::check(&file)
                    .map_err(|why| format!("frames: a move this body cannot play: {why}"))?;
            }
            // Any `within_ms`: the body's caps pace each step (the owner's
            // "no limits for developers", 30 Oct).
            Self::Joints { lease, targets, .. } => {
                lease_id(lease)?;
                let values = targets.values();
                if values.iter().all(Option::is_none) {
                    return Err("targets: at least one joint".to_owned());
                }
                if values
                    .iter()
                    .flatten()
                    .any(|value| !value.is_finite() || !(-180.0..=180.0).contains(value))
                {
                    return Err("targets: degrees from -180 to 180".to_owned());
                }
            }
            Self::Light {
                colour,
                level,
                rgb,
                pixels,
                brightness,
                pattern,
                fade_ms,
                for_ms,
            } => {
                let forms = [colour.is_some(), rgb.is_some(), pixels.is_some()];
                if forms.iter().filter(|given| **given).count() != 1 {
                    return Err("one of colour, rgb or pixels".to_owned());
                }
                if let Some(colour) = colour {
                    if !lamp_msgs::Light::COLOURS.contains(&colour.as_str()) {
                        return Err(format!(
                            "colour: one of {}",
                            lamp_msgs::Light::COLOURS.join(", ")
                        ));
                    }
                    if !level
                        .is_some_and(|level| (1..=lamp_msgs::Light::BRIGHTEST).contains(&level))
                    {
                        return Err(format!(
                            "level: from 1 to {}, with colour",
                            lamp_msgs::Light::BRIGHTEST
                        ));
                    }
                    if brightness.is_some() {
                        return Err(
                            "brightness goes with rgb or pixels; colour has level".to_owned()
                        );
                    }
                } else {
                    if level.is_some() || pattern.is_some() {
                        return Err("level and pattern go with colour".to_owned());
                    }
                    if brightness.is_some_and(|brightness| brightness > 100) {
                        return Err("brightness: from 0 to 100".to_owned());
                    }
                }
                if let Some(rgb) = rgb
                    && hex_rgb(rgb).is_none()
                {
                    return Err("rgb: #rrggbb".to_owned());
                }
                if let Some(pixels) = pixels {
                    if pixels.len() != lamp_msgs::LightRequest::PIXELS {
                        return Err(format!(
                            "pixels: {} of them, one per pixel of the ring",
                            lamp_msgs::LightRequest::PIXELS
                        ));
                    }
                    if pixels.iter().any(|pixel| hex_rgb(pixel).is_none()) {
                        return Err("pixels: each #rrggbb".to_owned());
                    }
                }
                if let Some(fade) = fade_ms
                    && !lamp_msgs::LightRequest::FADE_MS.contains(fade)
                {
                    return Err(format!(
                        "fade_ms: from {} to {}",
                        lamp_msgs::LightRequest::FADE_MS.start(),
                        lamp_msgs::LightRequest::FADE_MS.end()
                    ));
                }
                held_for(*for_ms, HOLD_MAX_MS)?;
            }
            Self::Say { text, .. } => words("text", text)?,
            // A turn's prompt is a say's words.
            Self::TurnStart {
                prompt: Some(prompt),
            } => words("prompt", prompt)?,
            Self::TurnStart { prompt: None }
            | Self::TurnEnd {}
            | Self::ListenSet { .. }
            | Self::Stop {}
            | Self::SkillList {}
            | Self::SoundList {}
            | Self::CameraSnapshot {} => {}
            Self::Volume { to, step, mute } => {
                let asked = usize::from(to.is_some())
                    + usize::from(step.is_some())
                    + usize::from(mute.is_some());
                if asked != 1 {
                    return Err("one of to, step or mute".to_owned());
                }
                if to.is_some_and(|to| to > 100) {
                    return Err("to: from 0 to 100".to_owned());
                }
            }
        }
        Ok(())
    }
}

/// Words the lamp says, a `say`'s `text` or a turn's `prompt`: some letter
/// or digit, at most what the voice takes ([`SAY_MAX_BYTES`]), and no
/// control character, a newline or tab included, as the voice refuses them
/// (lamp-voice's say.rs).
fn words(field: &str, text: &str) -> Result<(), String> {
    if !text.chars().any(char::is_alphanumeric) || text.chars().any(char::is_control) {
        return Err(format!("{field}: words, no control characters"));
    }
    if text.len() > SAY_MAX_BYTES {
        return Err(format!(
            "{field}: at most {SAY_MAX_BYTES} bytes as the voice takes it"
        ));
    }
    Ok(())
}

/// Which way a `volume` step goes: the voice's own step (`volume_up`,
/// `volume_down`), within the lamp's floor and ceiling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VolumeStep {
    Up,
    Down,
}

/// A request: its id, as its reply names it, and its operation.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub id: u64,
    pub op: Op,
}

/// Why a frame was refused before its op could run: the reply to send, with
/// the request's id when it could be read.
#[derive(Clone, Debug, PartialEq)]
pub struct Refused {
    pub re: Option<u64>,
    pub error: Error,
}

/// One frame read as a request: a JSON object with a whole-number `id`, a
/// known `op` and only that op's fields, passing [`Op::check`].
pub fn parse(frame: &str) -> Result<Request, Refused> {
    let bad = |re, why: String| Refused {
        re,
        error: Error::new(ErrorCode::BadRequest, why),
    };
    let mut object: Map<String, Value> = match serde_json::from_str(frame) {
        Ok(Value::Object(object)) => object,
        Ok(_) => return Err(bad(None, "a request is a JSON object".to_owned())),
        Err(error) => return Err(bad(None, format!("not JSON: {error}"))),
    };
    let id = match object.remove("id") {
        Some(Value::Number(id)) if id.is_u64() => id.as_u64(),
        _ => return Err(bad(None, "id: a whole number, 0 or more".to_owned())),
    };
    let op: Op = serde_json::from_value(Value::Object(object))
        .map_err(|error| bad(id, error.to_string()))?;
    op.check().map_err(|why| bad(id, why))?;
    Ok(Request {
        id: id.unwrap_or_default(),
        op,
    })
}

/// Why a request was refused, by code (docs/control.md, "Errors").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// Not paired, revoked, or a bad signature: the connection closes.
    Unpaired,
    /// `hello` not said yet on this connection.
    NoHello,
    /// The owner did not grant it.
    NoGrant,
    /// It needs a lease the client does not hold.
    NoLease,
    /// The lease it names was lost (the why says how).
    LeaseLost,
    /// A higher priority holds the part.
    Busy,
    /// The part's own check refused it (its why, as the part said it).
    Refused,
    QuietHours,
    Private,
    Paused,
    Asleep,
    /// Over its rate: `retry_ms` says when.
    Rate,
    BadRequest,
    /// Developer access is off in the app.
    Off,
}

/// An error's body: its code and why, and when to try again for `rate`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Error {
    pub code: ErrorCode,
    pub why: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_ms: Option<u64>,
}

impl Error {
    pub fn new(code: ErrorCode, why: impl Into<String>) -> Self {
        Self {
            code,
            why: why.into(),
            retry_ms: None,
        }
    }
}

/// A reply that worked: `{"re": N, "ok": true}` and the fields of `with`
/// (an object, or nothing).
pub fn ok(re: u64, with: Value) -> Value {
    let mut reply = Map::new();
    reply.insert("re".to_owned(), json!(re));
    reply.insert("ok".to_owned(), json!(true));
    if let Value::Object(fields) = with {
        for (key, value) in fields {
            if key != "re" && key != "ok" {
                reply.insert(key, value);
            }
        }
    }
    Value::Object(reply)
}

/// A refusal: `{"re": N, "ok": false, "error": {...}}`; `re` is null when
/// the request's id could not be read.
pub fn refused(re: Option<u64>, error: &Error) -> Value {
    json!({"re": re, "ok": false, "error": error})
}

/// A lease's state, as its events say it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeaseState {
    Held,
    Suspended,
    Lost,
}

/// Why a lease was suspended or lost.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeaseWhy {
    /// Someone began talking to the lamp: suspended, held again after.
    Conversation,
    /// A hand on the head.
    Hand,
    /// "Stop" heard.
    StopHeard,
    /// The privacy switch.
    Private,
    Paused,
    Asleep,
    /// Not renewed in time, or past the longest a take may last.
    Expired,
    /// The owner's app took the part.
    TakenByOwnerApp,
    /// The owner revoked the client.
    Revoked,
    /// Released by the client, or its connection closed; for `body`, also
    /// when the body refused the door's hold or ended it with a why the door
    /// doesn't know, so the lamp's own life is not standing back (Sixth's
    /// P3 on #3915).
    Released,
    /// The body gave way under load and let go (its torque off after a
    /// slump): moves stopped, and taking the lease straight back is no help
    /// (Ten's #3820, agreed on #3504 at 16:3x).
    Slumped,
}

/// A lease's event to its holder.
pub fn lease_event(lease: &str, state: LeaseState, why: Option<LeaseWhy>) -> Value {
    let mut event = json!({"ev": "lease", "lease": lease, "state": state});
    if let Some(why) = why {
        event["why"] = json!(why);
    }
    event
}

/// Whether `text` is standard base64 of exactly `bytes` bytes, padded.
/// A skill's name as the skill format has it: 1 to 64 of `a-z`, `0-9`
/// and `-`.
fn skill_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > SKILL_NAME_MAX_CHARS
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(format!(
            "name: a skill's, 1 to {SKILL_NAME_MAX_CHARS} of a-z, 0-9 and -"
        ));
    }
    Ok(())
}

fn base64_of(text: &str, bytes: usize) -> bool {
    let length = bytes.div_ceil(3) * 4;
    let padding = (3 - bytes % 3) % 3;
    text.len() == length
        && text.bytes().enumerate().all(|(index, byte)| {
            if index >= length - padding {
                byte == b'='
            } else {
                byte.is_ascii_alphanumeric() || byte == b'+' || byte == b'/'
            }
        })
}

/// `c_` and 1 to 32 lowercase hex digits.
fn client_id(text: &str) -> bool {
    text.strip_prefix("c_").is_some_and(|hex| {
        (1..=32).contains(&hex.len())
            && hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

/// `l_` and 1 to 20 digits.
fn lease_id(text: &str) -> Result<(), String> {
    text.strip_prefix("l_")
        .is_some_and(|digits| {
            (1..=20).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit())
        })
        .then_some(())
        .ok_or_else(|| "lease: the id lease.take gave, l_ and digits".to_owned())
}

/// A `#rrggbb` colour's red, green and blue, if it is one.
pub fn hex_rgb(text: &str) -> Option<[u8; 3]> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok();
    Some([channel(0)?, channel(2)?, channel(4)?])
}

fn held_for(ms: u32, max_ms: u32) -> Result<(), String> {
    (1..=max_ms)
        .contains(&ms)
        .then_some(())
        .ok_or_else(|| format!("for_ms: from 1 to {max_ms}"))
}

/// A lowercase name, as skills', moves' and gestures' are: a letter, then
/// at most 63 lowercase letters, digits, `_` or `-`.
/// A clip's name: a plain name, and never the lamp's own (`hum`).
fn clip_name(name: &str) -> Result<(), String> {
    plain_name(name, "name")?;
    if SOUNDS.contains(&name) {
        return Err(format!("name: {name} is the lamp's own sound"));
    }
    Ok(())
}

fn plain_name(text: &str, what: &str) -> Result<(), String> {
    let mut bytes = text.bytes();
    let first = bytes.next().is_some_and(|b| b.is_ascii_lowercase());
    (first
        && text.len() <= 64
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-'))
    .then_some(())
    .ok_or_else(|| {
        format!("{what}: a lowercase letter, then lowercase letters, digits, _ or -, at most 64")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    const SIG: &str =
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";

    fn op(frame: &str) -> Op {
        parse(frame)
            .unwrap_or_else(|refused| panic!("{frame}: {refused:?}"))
            .op
    }

    fn why(frame: &str) -> (Option<u64>, String) {
        let refused = parse(frame).expect_err(frame);
        assert_eq!(refused.error.code, ErrorCode::BadRequest, "{frame}");
        (refused.re, refused.error.why)
    }

    #[test]
    fn every_op_of_the_design_reads() {
        let frames = [
            format!(r#"{{"id":1,"op":"pair","name":"Dev's laptop","key":"{KEY}","proof":"{KEY}"}}"#),
            format!(r#"{{"id":1,"op":"hello","client":"c_3f9a","sig":"{SIG}"}}"#),
            r#"{"id":2,"op":"subscribe","events":["person.arrived","touch","phrase.heard"],"phrases":["lights please","good night lamp"]}"#.to_owned(),
            r#"{"id":3,"op":"get","state":["presence","light","lamp"]}"#.to_owned(),
            r#"{"id":4,"op":"unsubscribe","events":["touch"]}"#.to_owned(),
            r#"{"id":5,"op":"lease.take","parts":["body","light"],"for_ms":30000,"why":"desk robot demo"}"#.to_owned(),
            r#"{"id":6,"op":"lease.renew","lease":"l_81","for_ms":30000}"#.to_owned(),
            r#"{"id":7,"op":"lease.release","lease":"l_81"}"#.to_owned(),
            r#"{"id":8,"op":"express","feeling":"glad","strength":0.6}"#.to_owned(),
            r#"{"id":9,"op":"gesture","name":"nod"}"#.to_owned(),
            r#"{"id":10,"op":"look","at":{"yaw":-10,"pitch":5}}"#.to_owned(),
            r#"{"id":10,"op":"look","at":"face"}"#.to_owned(),
            r#"{"id":11,"op":"move","step_ms":500,"frames":[[0,0,0,0,0],[20,0,30,0,0],[20,0,30,0,0],[0,0,0,0,0]]}"#.to_owned(),
            r#"{"id":12,"op":"joints","lease":"l_81","targets":{"pitch":4.0,"yaw":-12.0},"within_ms":300}"#.to_owned(),
            r#"{"id":13,"op":"light","colour":"warm","level":3,"pattern":"breathe","for_ms":10000}"#.to_owned(),
            r#"{"id":14,"op":"say","text":"Build passed."}"#.to_owned(),
            r#"{"id":15,"op":"sound","name":"hum"}"#.to_owned(),
            r#"{"id":15,"op":"sound","name":"chime"}"#.to_owned(),
            format!(r#"{{"id":15,"op":"sound.upload","name":"chime","sha256":"{}","size":4,"part":0,"parts":1,"data":"AAAA"}}"#, "0".repeat(64)),
            r#"{"id":15,"op":"sound.list"}"#.to_owned(),
            r#"{"id":15,"op":"sound.remove","name":"chime"}"#.to_owned(),
            r#"{"id":16,"op":"play","name":"wave-hello"}"#.to_owned(),
            r#"{"id":17,"op":"stop"}"#.to_owned(),
        ];
        for frame in &frames {
            let read = parse(frame).unwrap_or_else(|refused| panic!("{frame}: {refused:?}"));
            // What it reads as writes back as the same op.
            let again = serde_json::to_value(&read.op).unwrap();
            assert_eq!(
                serde_json::from_value::<Op>(again).unwrap(),
                read.op,
                "{frame}"
            );
        }
        assert_eq!(
            op(r#"{"id":8,"op":"express","feeling":"low"}"#),
            Op::Express {
                feeling: "low".to_owned(),
                strength: 0.5
            }
        );
        assert_eq!(
            op(r#"{"id":10,"op":"look","at":"ahead"}"#),
            Op::Look {
                at: LookAt::Target(LookTarget::Ahead)
            }
        );
    }

    /// Ten's finding on #3997 (lamp-a0ae, 29 Oct): a 600 ms move was taken
    /// as `{"to":"body"}` and then dropped by the body. The door now holds a
    /// move to the body's own check and says the body's words.
    #[test]
    fn a_move_the_body_would_drop_is_refused_with_the_bodys_words() {
        let (re, words) = why(
            r#"{"id":4,"op":"move","step_ms":200,"frames":[[0,0,0,0,0],[20,0,0,0,0],[20,0,0,0,0],[0,0,0,0,0]]}"#,
        );
        assert_eq!(re, Some(4));
        assert!(
            words.contains("a move this body cannot play: it lasts 600 ms, not 1500 to 10000"),
            "{words}"
        );
        assert!(why(r#"{"id":5,"op":"move","step_ms":1000,"frames":[[0,0,0,0,0],[0,0,151,0,0],[0,0,0,0,0]]}"#).1.contains("farther than 150"));
        assert!(
            why(r#"{"id":6,"op":"move","step_ms":20,"frames":[[0,0,0,0,0],[0,0,0,0,0]]}"#)
                .1
                .contains("step_ms: from 40 to 1000")
        );
    }

    #[test]
    fn a_field_it_does_not_know_or_a_bad_frame_is_refused_with_its_id_when_read() {
        assert_eq!(
            why(r#"{"id":9,"op":"gesture","name":"nod","speed":3}"#).0,
            Some(9)
        );
        assert_eq!(why(r#"{"id":9,"op":"dance"}"#).0, Some(9));
        assert_eq!(why(r#"{"op":"stop"}"#).0, None);
        assert_eq!(why(r#"{"id":-1,"op":"stop"}"#).0, None);
        assert_eq!(why(r#"{"id":1.5,"op":"stop"}"#).0, None);
        assert_eq!(why(r#"[1,2]"#).0, None);
        assert_eq!(why("not json").0, None);
        assert_eq!(
            why(r#"{"id":3,"op":"joints","lease":"l_1","targets":{"knee":3},"within_ms":1}"#).0,
            Some(3)
        );
    }

    /// The owner's "no limits for developers" (30 Oct): what the door
    /// once refused for policy alone is taken, and the body's own bounds
    /// and the frame's size still hold it.
    #[test]
    fn limits_kept_only_for_policy_are_lifted() {
        let accepted = [
            r#"{"id":1,"op":"gesture","name":"idle"}"#.to_owned(),
            r#"{"id":1,"op":"gesture","name":"scanning"}"#.to_owned(),
            r#"{"id":1,"op":"look","at":{"yaw":270,"pitch":-120}}"#.to_owned(),
            r#"{"id":1,"op":"joints","lease":"l_1","targets":{"pitch":4},"within_ms":10000}"#
                .to_owned(),
            r#"{"id":1,"op":"lease.take","parts":["body"],"for_ms":600000}"#.to_owned(),
            r#"{"id":1,"op":"lease.renew","lease":"l_1","for_ms":600000}"#.to_owned(),
        ];
        for frame in &accepted {
            assert!(parse(frame).is_ok(), "{frame}: {:?}", parse(frame));
        }
        // A move as long as the body plays (10 s at a 40 ms step, 251
        // frames) is taken; one longer, the body's check refuses.
        let still = |n: usize| {
            let frames = vec!["[0,0,0,0,0]"; n].join(",");
            format!(r#"{{"id":1,"op":"move","step_ms":40,"frames":[{frames}]}}"#)
        };
        assert!(parse(&still(251)).is_ok());
        assert!(why(&still(252)).1.contains("not 1500 to 10000"));
    }

    /// A say is held to the voice's own length, 900 UTF-8 bytes
    /// (`lamp_voice::say::MOST_WORDS_BYTES`), counted in bytes, not
    /// characters.
    #[test]
    fn a_say_is_at_most_the_900_bytes_the_voice_takes() {
        let say = |text: &str| format!(r#"{{"id":1,"op":"say","text":"{text}"}}"#);
        assert!(parse(&say(&"a".repeat(SAY_MAX_BYTES))).is_ok());
        let (re, words) = why(&say(&"a".repeat(SAY_MAX_BYTES + 1)));
        assert_eq!(re, Some(1));
        assert!(
            words.contains("text: at most 900 bytes as the voice takes it"),
            "{words}"
        );
        // "ế" is three bytes: 300 of them are 900 bytes, 301 are over.
        assert_eq!("ế".len(), 3);
        assert!(parse(&say(&"ế".repeat(300))).is_ok());
        // Any control character, a newline or tab too, as the voice refuses.
        for control in [r"\n", r"\t", r"\r", r"\u0007"] {
            assert!(
                why(&say(&format!("two{control}lines")))
                    .1
                    .contains("text: words, no control characters"),
                "{control}"
            );
        }
        assert!(why(&say(&"ế".repeat(301))).1.contains("at most 900 bytes"));
        // A turn's prompt is a say's words, held the same way.
        let turn = |prompt: &str| format!(r#"{{"id":1,"op":"turn.start","prompt":"{prompt}"}}"#);
        assert!(parse(&turn(&"ế".repeat(300))).is_ok());
        assert!(
            why(&turn(&"ế".repeat(301)))
                .1
                .contains("prompt: at most 900 bytes as the voice takes it")
        );
        assert!(
            why(&turn(r"two\nlines"))
                .1
                .contains("prompt: words, no control characters")
        );
    }

    #[test]
    fn each_ops_own_limits_refuse_it() {
        let refused = [
            r#"{"id":1,"op":"say","text":""}"#.to_owned(),
            r#"{"id":1,"op":"say","text":"..."}"#.to_owned(),
            r#"{"id":1,"op":"say","text":"hi\u0007"}"#.to_owned(),
            r#"{"id":1,"op":"light","colour":"magenta","level":3,"for_ms":1000}"#.to_owned(),
            r#"{"id":1,"op":"light","colour":"warm","level":0,"for_ms":1000}"#.to_owned(),
            r#"{"id":1,"op":"light","colour":"warm","level":6,"for_ms":1000}"#.to_owned(),
            r#"{"id":1,"op":"light","colour":"warm","level":3,"for_ms":30001}"#.to_owned(),
            r#"{"id":1,"op":"light","colour":"warm","level":3,"for_ms":0}"#.to_owned(),
            r#"{"id":1,"op":"light","colour":"warm","level":3,"pattern":"strobe","for_ms":1000}"#
                .to_owned(),
            r#"{"id":1,"op":"lease.take","parts":[],"for_ms":1000}"#.to_owned(),
            r#"{"id":1,"op":"lease.take","parts":["arm"],"for_ms":1000}"#.to_owned(),
            r#"{"id":1,"op":"lease.renew","lease":"81","for_ms":1000}"#.to_owned(),
            r#"{"id":1,"op":"lease.take","parts":["body"],"for_ms":600001}"#.to_owned(),
            r#"{"id":1,"op":"lease.take","parts":["body"],"for_ms":0}"#.to_owned(),
            r#"{"id":1,"op":"lease.renew","lease":"l_1","for_ms":600001}"#.to_owned(),
            r#"{"id":1,"op":"express","feeling":"glad","strength":1.5}"#.to_owned(),
            r#"{"id":1,"op":"express","feeling":"Glad"}"#.to_owned(),
            r#"{"id":1,"op":"gesture","name":"../nod"}"#.to_owned(),
            // The hum or a clip's plain name: never a path. A recorded
            // notice's name (wifi_lost) is only ever a clip of the
            // computer's own, which the voice reads from its folder.
            r#"{"id":1,"op":"sound","name":"../hum"}"#.to_owned(),
            r#"{"id":1,"op":"sound","name":"/etc/hosts"}"#.to_owned(),
            r#"{"id":1,"op":"sound","name":"Chime"}"#.to_owned(),
            // A clip: never named as the lamp's own, its size within a
            // 30 s clip's, its parts in range, its SHA-256 64 hex.
            format!(r#"{{"id":1,"op":"sound.upload","name":"hum","sha256":"{}","size":4,"part":0,"parts":1,"data":"AAAA"}}"#, "0".repeat(64)),
            format!(r#"{{"id":1,"op":"sound.upload","name":"a","sha256":"{}","size":{},"part":0,"parts":1,"data":"AAAA"}}"#, "0".repeat(64), crate::sounds::CLIP_MOST_BYTES + 1),
            format!(r#"{{"id":1,"op":"sound.upload","name":"a","sha256":"{}","size":4,"part":1,"parts":1,"data":"AAAA"}}"#, "0".repeat(64)),
            format!(r#"{{"id":1,"op":"sound.upload","name":"a","sha256":"{}","size":4,"part":0,"parts":65,"data":"AAAA"}}"#, "0".repeat(64)),
            r#"{"id":1,"op":"sound.upload","name":"a","sha256":"00","size":4,"part":0,"parts":1,"data":"AAAA"}"#.to_owned(),
            r#"{"id":1,"op":"sound.remove","name":"hum"}"#.to_owned(),
            r#"{"id":1,"op":"look","at":{"yaw":10,"pitch":5,"roll":30}}"#.to_owned(),
            r#"{"id":1,"op":"stop","lease":"l_1"}"#.to_owned(),
            r#"{"id":1,"op":"move","step_ms":5,"frames":[[0,0,0,0,0],[0,0,0,0,0]]}"#.to_owned(),
            // What the body would drop: 600 ms, a 20 ms step, too far, too fast
            // (lamp_body::moves::check; Ten's finding on #3997).
            r#"{"id":1,"op":"move","step_ms":200,"frames":[[0,0,0,0,0],[20,0,0,0,0],[20,0,0,0,0],[0,0,0,0,0]]}"#.to_owned(),
            r#"{"id":1,"op":"move","step_ms":20,"frames":[[0,0,0,0,0],[0,0,0,0,0]]}"#.to_owned(),
            r#"{"id":1,"op":"move","step_ms":1000,"frames":[[0,0,0,0,0],[0,0,151,0,0],[0,0,0,0,0]]}"#.to_owned(),
            r#"{"id":1,"op":"move","step_ms":100,"frames":[[0,0,0,0,0],[0,0,0,0,0],[0,0,0,0,0],[0,0,0,0,0],[0,0,0,0,0],[0,0,0,0,0],[0,0,0,0,0],[0,0,0,0,300],[0,0,0,0,300],[0,0,0,0,300],[0,0,0,0,300],[0,0,0,0,300],[0,0,0,0,300],[0,0,0,0,300],[0,0,0,0,300],[0,0,0,0,0]]}"#.to_owned(),
            r#"{"id":1,"op":"move","step_ms":40,"frames":[[0,0,0,0,0]]}"#.to_owned(),
            r#"{"id":1,"op":"move","step_ms":40,"frames":[[10,0,0,0,0],[0,0,0,0,0]]}"#.to_owned(),
            r#"{"id":1,"op":"move","step_ms":40,"frames":[[0,0,0,0,0],[0,0,0,0]]}"#.to_owned(),
            r#"{"id":1,"op":"joints","lease":"l_1","targets":{},"within_ms":100}"#.to_owned(),
            r#"{"id":1,"op":"joints","lease":"l_1","targets":{"pitch":400},"within_ms":100}"#
                .to_owned(),
            r#"{"id":1,"op":"subscribe","events":[]}"#.to_owned(),
            r#"{"id":1,"op":"subscribe","events":["phrase.heard"]}"#.to_owned(),
            r#"{"id":1,"op":"subscribe","events":["touch"],"phrases":["hi"]}"#.to_owned(),
            r#"{"id":1,"op":"subscribe","events":["camera.frame"]}"#.to_owned(),
            r#"{"id":1,"op":"get","state":["transcript"]}"#.to_owned(),
            format!(r#"{{"id":1,"op":"pair","name":"","key":"{KEY}","proof":"{KEY}"}}"#),
            format!(r#"{{"id":1,"op":"pair","name":"x","key":"short","proof":"{KEY}"}}"#),
            format!(r#"{{"id":1,"op":"hello","client":"3f9a","sig":"{SIG}"}}"#),
            format!(r#"{{"id":1,"op":"hello","client":"c_3f9a","sig":"{KEY}"}}"#),
        ];
        for frame in &refused {
            assert_eq!(why(frame).0, Some(1), "{frame}");
        }
        let phrases: Vec<String> = (0..=PHRASES_MAX)
            .map(|n| format!("\"phrase number {n}\""))
            .collect();
        let frame = format!(
            r#"{{"id":1,"op":"subscribe","events":["phrase.heard"],"phrases":[{}]}}"#,
            phrases.join(",")
        );
        assert!(why(&frame).1.contains("at most 16"), "{:?}", why(&frame));
        // Fourteen's P2 and harness-reviewer's P3 on #3868: 8 letters at
        // least, never a single common word or the commonest pairs.
        let subscribe = |phrase: &str| {
            format!(
                r#"{{"id":1,"op":"subscribe","events":["phrase.heard"],"phrases":["{phrase}"]}}"#
            )
        };
        for short in [
            "a", "yes", "the", "lights", "ok!", "don't", "it's", "is a", "it is", "ok lamp",
        ] {
            assert!(why(&subscribe(short)).1.contains("too short"), "{short}");
        }
        for fine in [
            "lights please",
            "good night lamp",
            "password",
            "lights on",
            "don't stop",
        ] {
            assert!(parse(&subscribe(fine)).is_ok(), "{fine}");
        }
    }

    /// Fourteen's note on #3869: the pairing screen's words are pinned,
    /// `skills`' above all, which says what it puts on keeps running.
    #[test]
    fn the_pairing_screen_says_each_grant_as_pinned() {
        let shown: Vec<(Value, &str)> = [
            Grant::Body,
            Grant::Light,
            Grant::Speak,
            Grant::Look,
            Grant::Hear,
            Grant::Sound,
            Grant::Skills,
            Grant::Air,
            Grant::Device,
            Grant::Camera,
            Grant::Microphone,
        ]
        .into_iter()
        .map(|grant| (json!(grant), grant.shown()))
        .collect();
        assert_eq!(
            shown,
            [
                (json!("body"), "can move the lamp's body"),
                (json!("light"), "can change the light"),
                (json!("speak"), "can speak and play sounds"),
                (json!("look"), "can tell when someone is there, never who"),
                (
                    json!("hear"),
                    "can hear its own phrases, never the words around them"
                ),
                (json!("sound"), "can hear kinds of sound, never words"),
                (
                    json!("skills"),
                    "can put skills on the lamp that run on their own"
                ),
                (
                    json!("air"),
                    "can read the air sensor: particles, carbon dioxide and humidity, which can show when someone is in the room"
                ),
                (
                    json!("device"),
                    "can read the lamp's processor, memory, disk and temperature"
                ),
                (
                    json!("camera"),
                    "can see what the lamp's camera sees, never while it is private"
                ),
                (
                    json!("microphone"),
                    "can hear what the lamp's microphone hears, never while it is private"
                ),
            ]
        );
    }

    #[test]
    fn a_camera_stream_may_ask_up_to_30_frames_a_second() {
        for fps in [1, 30] {
            let frame = format!(r#"{{"id":1,"op":"camera.stream","on":true,"fps":{fps}}}"#);
            assert_eq!(op(&frame).check(), Ok(()), "{fps}");
        }
        for fps in [0, 31] {
            let frame = format!(r#"{{"id":1,"op":"camera.stream","on":true,"fps":{fps}}}"#);
            assert_eq!(why(&frame).1, "fps: 1 to 30", "{fps}");
        }
    }

    #[test]
    fn grants_and_leases_each_op_needs() {
        let needs = |frame: &str| op(frame).needs();
        assert_eq!(needs(r#"{"id":1,"op":"stop"}"#), []);
        assert_eq!(
            needs(r#"{"id":1,"op":"gesture","name":"nod"}"#),
            [Grant::Body]
        );
        assert_eq!(
            needs(r#"{"id":1,"op":"express","feeling":"glad"}"#),
            [Grant::Body, Grant::Light]
        );
        assert_eq!(
            needs(r#"{"id":1,"op":"look","at":"face"}"#),
            [Grant::Body, Grant::Look]
        );
        assert_eq!(
            needs(r#"{"id":1,"op":"look","at":"hand"}"#),
            [Grant::Body, Grant::Look]
        );
        assert_eq!(needs(r#"{"id":1,"op":"look","at":"ahead"}"#), [Grant::Body]);
        assert_eq!(needs(r#"{"id":1,"op":"say","text":"Hi."}"#), [Grant::Speak]);
        assert_eq!(
            needs(
                r#"{"id":1,"op":"subscribe","events":["person.arrived","sound.heard","touch","phrase.heard"],"phrases":["lights please"]}"#
            ),
            [Grant::Look, Grant::Hear, Grant::Sound]
        );
        assert_eq!(
            needs(r#"{"id":1,"op":"get","state":["presence","mood"]}"#),
            [Grant::Look]
        );
        assert_eq!(
            needs(r#"{"id":1,"op":"lease.take","parts":["voice","body"],"for_ms":1}"#),
            [Grant::Body, Grant::Speak]
        );
        let lease = |frame: &str| op(frame).needs_lease();
        assert_eq!(lease(r#"{"id":1,"op":"gesture","name":"nod"}"#), None);
        assert_eq!(lease(r#"{"id":1,"op":"express","feeling":"glad"}"#), None);
        assert_eq!(
            lease(r#"{"id":1,"op":"look","at":"face"}"#),
            Some(Part::Body)
        );
        assert_eq!(
            lease(r#"{"id":1,"op":"play","name":"wave"}"#),
            Some(Part::Body)
        );
        assert_eq!(
            lease(r#"{"id":1,"op":"light","colour":"warm","level":3,"for_ms":1000}"#),
            None
        );
        assert!(
            !op(&format!(
                r#"{{"id":1,"op":"hello","client":"c_1","sig":"{SIG}"}}"#
            ))
            .needs_hello()
        );
        assert!(op(r#"{"id":1,"op":"stop"}"#).needs_hello());
    }

    #[test]
    fn replies_and_events_are_shaped_as_the_design_says() {
        assert_eq!(
            ok(5, json!({"lease": "l_81", "re": 99, "ok": false})),
            json!({"re": 5, "ok": true, "lease": "l_81"})
        );
        assert_eq!(ok(5, Value::Null), json!({"re": 5, "ok": true}));
        let mut rate = Error::new(ErrorCode::Rate, "say: 6 a minute");
        rate.retry_ms = Some(4_000);
        assert_eq!(
            refused(Some(14), &rate),
            json!({"re": 14, "ok": false, "error": {"code": "rate", "why": "say: 6 a minute", "retry_ms": 4000}})
        );
        assert_eq!(
            refused(None, &Error::new(ErrorCode::NoGrant, "body")),
            json!({"re": null, "ok": false, "error": {"code": "no_grant", "why": "body"}})
        );
        assert_eq!(
            lease_event("l_81", LeaseState::Suspended, Some(LeaseWhy::Conversation)),
            json!({"ev": "lease", "lease": "l_81", "state": "suspended", "why": "conversation"})
        );
        assert_eq!(
            lease_event("l_81", LeaseState::Held, None),
            json!({"ev": "lease", "lease": "l_81", "state": "held"})
        );
    }

    #[test]
    fn base64_lengths_are_exact() {
        assert!(base64_of(KEY, 32));
        assert!(base64_of(SIG, 64));
        assert!(!base64_of(SIG, 32));
        assert!(!base64_of(
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            32
        ));
        assert!(!base64_of(
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA*=",
            32
        ));
    }

    /// A light is one of a named colour at a level, a computer's own `rgb`,
    /// or its `pixels`, one per pixel of the ring, with a brightness.
    #[test]
    fn a_light_takes_one_of_colour_rgb_or_pixels() {
        let checked = |frame: String| parse(&frame).map(|request| request.op.check());
        let pixels = |n: usize, each: &str| serde_json::to_string(&vec![each; n]).unwrap();
        for good in [
            r#"{"id":1,"op":"light","colour":"warm","level":3,"for_ms":1000}"#.to_owned(),
            r##"{"id":1,"op":"light","rgb":"#ff8000","for_ms":1000}"##.to_owned(),
            r##"{"id":1,"op":"light","rgb":"#FF8000","brightness":0,"fade_ms":1000,"for_ms":1000}"##.to_owned(),
            format!(r#"{{"id":1,"op":"light","pixels":{},"brightness":100,"for_ms":1000}}"#, pixels(32, "#010203")),
        ] {
            assert_eq!(checked(good.clone()), Ok(Ok(())), "{good}");
        }
        for bad in [
            r#"{"id":1,"op":"light","for_ms":1000}"#.to_owned(),
            r##"{"id":1,"op":"light","colour":"warm","level":3,"rgb":"#ff0000","for_ms":1000}"##
                .to_owned(),
            r#"{"id":1,"op":"light","colour":"warm","for_ms":1000}"#.to_owned(),
            r#"{"id":1,"op":"light","colour":"warm","level":3,"brightness":50,"for_ms":1000}"#
                .to_owned(),
            r##"{"id":1,"op":"light","rgb":"#ff0000","level":3,"for_ms":1000}"##.to_owned(),
            r##"{"id":1,"op":"light","rgb":"#ff0000","pattern":"breathe","for_ms":1000}"##
                .to_owned(),
            r##"{"id":1,"op":"light","rgb":"#ff0000","brightness":101,"for_ms":1000}"##.to_owned(),
            r#"{"id":1,"op":"light","rgb":"ff0000","for_ms":1000}"#.to_owned(),
            r##"{"id":1,"op":"light","rgb":"#ff00","for_ms":1000}"##.to_owned(),
            r##"{"id":1,"op":"light","rgb":"#ff000g","for_ms":1000}"##.to_owned(),
            format!(
                r#"{{"id":1,"op":"light","pixels":{},"for_ms":1000}}"#,
                pixels(31, "#010203")
            ),
            format!(
                r#"{{"id":1,"op":"light","pixels":{},"for_ms":1000}}"#,
                pixels(32, "red")
            ),
            r##"{"id":1,"op":"light","rgb":"#ff0000","fade_ms":999,"for_ms":1000}"##.to_owned(),
            r##"{"id":1,"op":"light","rgb":"#ff0000","for_ms":30001}"##.to_owned(),
        ] {
            assert!(!matches!(checked(bad.clone()), Ok(Ok(()))), "{bad}");
        }
        assert_eq!(hex_rgb("#0a0B0c"), Some([10, 11, 12]));
    }

    #[test]
    fn a_volume_asks_exactly_one_thing_and_needs_speak() {
        let checked = |frame: &str| parse(frame).map(|request| request.op.check());
        assert_eq!(checked(r#"{"id":1,"op":"volume","to":0}"#), Ok(Ok(())));
        assert_eq!(
            checked(r#"{"id":1,"op":"volume","step":"down"}"#),
            Ok(Ok(()))
        );
        assert_eq!(
            checked(r#"{"id":1,"op":"volume","mute":false}"#),
            Ok(Ok(()))
        );
        for refused in [
            r#"{"id":1,"op":"volume"}"#,
            r#"{"id":1,"op":"volume","to":101}"#,
            r#"{"id":1,"op":"volume","to":30,"mute":true}"#,
            r#"{"id":1,"op":"volume","step":"up","mute":true}"#,
        ] {
            assert!(!matches!(checked(refused), Ok(Ok(()))), "{refused}");
        }
        assert!(parse(r#"{"id":1,"op":"volume","step":"sideways"}"#).is_err());
        let op = parse(r#"{"id":1,"op":"volume","to":30}"#).unwrap().op;
        assert_eq!(op.needs(), vec![Grant::Speak]);
        assert_eq!(StateKind::Volume.needs(), Some(Grant::Speak));
        assert_eq!(EventKind::VolumeChanged.needs(), Some(Grant::Speak));
    }
}
