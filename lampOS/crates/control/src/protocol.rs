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

/// The longest `say`, in characters.
pub const SAY_MAX_CHARS: usize = 300;
/// How many phrases one client may listen for (`phrase.heard`).
pub const PHRASES_MAX: usize = 16;
/// The longest phrase, in characters.
pub const PHRASE_MAX_CHARS: usize = 64;
/// A phrase is at least two words, or one of at least this many letters, so
/// a program can't register single common words ("a", "yes") and probe the
/// room's speech word by word (Fourteen's P2 on #3868).
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
/// The longest a light request or one lease renewal holds, in ms: as the
/// ring's own light requests ([`lamp_msgs::LightRequest`]'s claims, ≤ 30 s).
pub const HOLD_MAX_MS: u32 = 30_000;
/// A move's step, in ms.
pub const STEP_MS: std::ops::RangeInclusive<u32> = 20..=1_000;
/// A move's frames: from a start to an end at the least.
pub const FRAMES: std::ops::RangeInclusive<usize> = 2..=500;
/// The longest a `joints` goal may take to reach, in ms.
pub const WITHIN_MAX_MS: u32 = 5_000;
/// The longest name a client gives itself at pairing, in characters.
pub const CLIENT_NAME_MAX_CHARS: usize = 64;

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
    /// `say` and `sound`.
    Speak,
    /// Presence: arrivals, leavings, facing, standing up (positions, never
    /// who).
    Look,
    /// Its own phrases heard, never the words around them, and the name.
    Hear,
    /// Sound classes heard.
    Sound,
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
}

impl EventKind {
    /// The grant a subscription to it needs, if any.
    pub fn needs(self) -> Option<Grant> {
        match self {
            Self::PersonArrived | Self::PersonLeft | Self::PersonStoodUp | Self::PersonFacing => {
                Some(Grant::Look)
            }
            Self::PhraseHeard | Self::NameHeard => Some(Grant::Hear),
            Self::SoundHeard => Some(Grant::Sound),
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
}

impl StateKind {
    /// The grant reading it needs, if any.
    pub fn needs(self) -> Option<Grant> {
        match self {
            Self::Presence => Some(Grant::Look),
            Self::Light | Self::Mood | Self::Lamp => None,
        }
    }
}

/// Where `look` turns.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LookAt {
    /// The face in view, the voice that called, or straight ahead.
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

/// Body clips a client may not ask for: `idle` is the lamp's own life, and
/// `scanning` a room sweep, which CLAUDE.md's rule 8 keeps for a person's
/// own ask (blue on #3504, 27 Oct).
pub const GESTURES_REFUSED: [&str; 2] = ["idle", "scanning"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LookTarget {
    Face,
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
    #[serde(rename = "light")]
    Light {
        colour: String,
        level: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pattern: Option<Pattern>,
        for_ms: u32,
    },
    #[serde(rename = "say")]
    Say { text: String },
    #[serde(rename = "sound")]
    Sound { name: String },
    /// Ends the client's own actions now. Needs no lease.
    #[serde(rename = "stop")]
    Stop {},
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
                LookAt::Target(LookTarget::Face) => vec![Grant::Body, Grant::Look],
                _ => vec![Grant::Body],
            },
            Self::Light { .. } => vec![Grant::Light],
            Self::Say { .. } | Self::Sound { .. } => vec![Grant::Speak],
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
                    let words = phrase_words(phrase).len();
                    let letters = phrase.chars().filter(|c| c.is_alphanumeric()).count();
                    if words < 2 && letters < PHRASE_MIN_LETTERS {
                        return Err(format!(
                            "phrases: {phrase:?} is too short: two words or more, or one of {PHRASE_MIN_LETTERS} letters"
                        ));
                    }
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
                held_for(*for_ms)?;
                if why.as_ref().is_some_and(|why| why.chars().count() > 200) {
                    return Err("why: at most 200 characters".to_owned());
                }
            }
            Self::LeaseRenew { lease, for_ms } => {
                lease_id(lease)?;
                held_for(*for_ms)?;
            }
            Self::LeaseRelease { lease } => lease_id(lease)?,
            Self::Express { feeling, strength } => {
                plain_name(feeling, "feeling")?;
                if !(0.0..=1.0).contains(strength) {
                    return Err("strength: from 0 to 1".to_owned());
                }
            }
            Self::Gesture { name } => {
                plain_name(name, "name")?;
                if GESTURES_REFUSED.contains(&name.as_str()) {
                    return Err(format!("name: {name} is the lamp's own, not a client's"));
                }
            }
            Self::Play { name } | Self::Sound { name } => {
                plain_name(name, "name")?;
            }
            Self::Look { at } => {
                if let LookAt::Angles(Angles { yaw, pitch }) = at
                    && !(yaw.is_finite()
                        && pitch.is_finite()
                        && (-180.0..=180.0).contains(yaw)
                        && (-90.0..=90.0).contains(pitch))
                {
                    return Err("at: yaw from -180 to 180, pitch from -90 to 90".to_owned());
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
                if !FRAMES.contains(&frames.len()) {
                    return Err(format!(
                        "frames: from {} to {}",
                        FRAMES.start(),
                        FRAMES.end()
                    ));
                }
                if frames.first() != Some(&[0; 5]) || frames.last() != Some(&[0; 5]) {
                    return Err("frames: start and end at home, all five 0".to_owned());
                }
            }
            Self::Joints {
                lease,
                targets,
                within_ms,
            } => {
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
                if *within_ms > WITHIN_MAX_MS {
                    return Err(format!("within_ms: at most {WITHIN_MAX_MS}"));
                }
            }
            Self::Light {
                colour,
                level,
                for_ms,
                ..
            } => {
                if !lamp_msgs::Light::COLOURS.contains(&colour.as_str()) {
                    return Err(format!(
                        "colour: one of {}",
                        lamp_msgs::Light::COLOURS.join(", ")
                    ));
                }
                if !(1..=lamp_msgs::Light::BRIGHTEST).contains(level) {
                    return Err(format!("level: from 1 to {}", lamp_msgs::Light::BRIGHTEST));
                }
                held_for(*for_ms)?;
            }
            Self::Say { text } => {
                let chars = text.chars().count();
                if !text.chars().any(char::is_alphanumeric)
                    || chars > SAY_MAX_CHARS
                    || text.chars().any(|c| c.is_control() && c != '\n')
                {
                    return Err(format!("text: words, at most {SAY_MAX_CHARS} characters"));
                }
            }
            Self::Stop {} => {}
        }
        Ok(())
    }
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
    /// Released by the client, or its connection closed.
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

fn held_for(ms: u32) -> Result<(), String> {
    (1..=HOLD_MAX_MS)
        .contains(&ms)
        .then_some(())
        .ok_or_else(|| format!("for_ms: from 1 to {HOLD_MAX_MS}"))
}

/// A lowercase name, as skills', moves' and gestures' are: a letter, then
/// at most 63 lowercase letters, digits, `_` or `-`.
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
            r#"{"id":11,"op":"move","step_ms":40,"frames":[[0,0,0,0,0],[20,0,30,0,0],[0,0,0,0,0]]}"#.to_owned(),
            r#"{"id":12,"op":"joints","lease":"l_81","targets":{"pitch":4.0,"yaw":-12.0},"within_ms":300}"#.to_owned(),
            r#"{"id":13,"op":"light","colour":"warm","level":3,"pattern":"breathe","for_ms":10000}"#.to_owned(),
            r#"{"id":14,"op":"say","text":"Build passed."}"#.to_owned(),
            r#"{"id":15,"op":"sound","name":"chime"}"#.to_owned(),
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

    #[test]
    fn each_ops_own_limits_refuse_it() {
        let refused = [
            r#"{"id":1,"op":"say","text":""}"#.to_owned(),
            r#"{"id":1,"op":"say","text":"..."}"#.to_owned(),
            format!(
                r#"{{"id":1,"op":"say","text":"{}"}}"#,
                "a".repeat(SAY_MAX_CHARS + 1)
            ),
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
            r#"{"id":1,"op":"express","feeling":"glad","strength":1.5}"#.to_owned(),
            r#"{"id":1,"op":"express","feeling":"Glad"}"#.to_owned(),
            r#"{"id":1,"op":"gesture","name":"../nod"}"#.to_owned(),
            r#"{"id":1,"op":"look","at":{"yaw":200,"pitch":0}}"#.to_owned(),
            r#"{"id":1,"op":"look","at":{"yaw":10,"pitch":5,"roll":30}}"#.to_owned(),
            r#"{"id":1,"op":"stop","lease":"l_1"}"#.to_owned(),
            r#"{"id":1,"op":"gesture","name":"idle"}"#.to_owned(),
            r#"{"id":1,"op":"gesture","name":"scanning"}"#.to_owned(),
            r#"{"id":1,"op":"move","step_ms":5,"frames":[[0,0,0,0,0],[0,0,0,0,0]]}"#.to_owned(),
            r#"{"id":1,"op":"move","step_ms":40,"frames":[[0,0,0,0,0]]}"#.to_owned(),
            r#"{"id":1,"op":"move","step_ms":40,"frames":[[10,0,0,0,0],[0,0,0,0,0]]}"#.to_owned(),
            r#"{"id":1,"op":"move","step_ms":40,"frames":[[0,0,0,0,0],[0,0,0,0]]}"#.to_owned(),
            r#"{"id":1,"op":"joints","lease":"l_1","targets":{},"within_ms":100}"#.to_owned(),
            r#"{"id":1,"op":"joints","lease":"l_1","targets":{"pitch":400},"within_ms":100}"#
                .to_owned(),
            r#"{"id":1,"op":"joints","lease":"l_1","targets":{"pitch":4},"within_ms":5001}"#
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
        // Fourteen's P2 on #3868: two words or one of 8 letters, never a
        // single common word.
        let subscribe = |phrase: &str| {
            format!(
                r#"{{"id":1,"op":"subscribe","events":["phrase.heard"],"phrases":["{phrase}"]}}"#
            )
        };
        for short in ["a", "yes", "the", "lights", "ok!", "don't", "it's"] {
            assert!(why(&subscribe(short)).1.contains("too short"), "{short}");
        }
        for fine in [
            "lights please",
            "good night lamp",
            "password",
            "ok lamp",
            "don't stop",
        ] {
            assert!(parse(&subscribe(fine)).is_ok(), "{fine}");
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
}
