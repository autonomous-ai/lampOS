//! What the lamp's bus says, as the door's events, lease signals and state
//! (docs/control.md, "Events"). Pure: each bus message goes in
//! ([`Watch::take`]) and what it means comes out; the server subscribes and
//! hands them on.
//!
//! - **Events,** by edge, never by every message: someone arrived (a face
//!   after none for [`ABSENT_FOR_US`]) or left (none for [`LEFT_AFTER_US`]),
//!   turned to or from the lamp, stood up; the lamp's name heard; a sound
//!   class rising past [`SOUND_BAR`]; a hand seen, moved, opened or closed,
//!   or gone, at most every [`HAND_EVERY_US`]; a touch or the button; the
//!   lamp talking, paused, private or asleep.
//! - **Words heard** go out only as [`Out::Heard`], for the server to match
//!   against each connection's own phrases: the words themselves never
//!   leave the lamp.
//! - **Lease signals:** talking suspends every lease, resting holds them
//!   again; a hand pushing the body, a stop from the head, a pause, the
//!   privacy switch or sleep loses them.
//! - **State,** for `get`: presence (counts, never who), the light, the
//!   inner state, and the lamp's own.

use serde_json::{Value, json};

use crate::{
    body_state::BodyState,
    protocol::{EventKind, LeaseWhy, Part},
};

/// A face counts as arrived after none for this long.
pub const ABSENT_FOR_US: u64 = 3_000_000;
/// Nobody in view this long is someone left.
pub const LEFT_AFTER_US: u64 = 5_000_000;
/// A sound class's score that counts as heard.
pub const SOUND_BAR: f64 = 0.5;
/// A privacy lease not handed on this long is private: a voice that stalls
/// stops renewing it, and the microphone is then closed to everyone else.
/// The bus thread hands a repeat on every [`REPEAT_EVERY_US`] and the check
/// runs every 250 ms, so that is 0.75 to 1.25 s after the last lease heard
/// (the Reviewer-lead's P3 on #4041).
pub const PRIVACY_LAPSE_US: u64 = 1_000_000;
/// The bus thread hands on a repeated state message at least this often, so
/// [`Watch::lapse`] knows the privacy lease is still renewed.
pub const REPEAT_EVERY_US: u64 = 250_000;
/// A hand not heard of this long is gone: `humans/hands/tracked` sends one
/// message per look that sees a hand, and none without one (Ten's topic).
pub const HAND_GONE_US: u64 = 1_000_000;
/// A hand is told at most this often, and only when it is news.
pub const HAND_EVERY_US: u64 = 250_000;
/// How far a hand moves, or grows, as a share of the picture, to be news.
pub const HAND_MOVED: f64 = 0.05;
/// A hand the tracker is less sure of than this isn't one.
pub const HAND_BAR: f64 = 0.5;
/// The sound classes told, never `speech`.
pub const SOUND_CLASSES: [&str; 4] = ["music", "laughter", "door", "footsteps"];
/// `air.changed` is told at most this often, as often as `air/state` comes.
pub const AIR_EVERY_US: u64 = 5_000_000;
/// A reading of the air or the board not heard again this long is no longer
/// known: both come every 5 s, and `board/state` means nothing after 15.
pub const READING_STALE_US: u64 = 15_000_000;
/// The room counts as unseen after this long with no `humans/faces/tracked`:
/// the eyes say their faces several times a second, nobody there included,
/// so a count this old may be frozen (the eyes stopped, restarted, or the
/// camera is out), and what it says of the room is unknown (the local
/// leader's P2 on the microphone).
pub const FACES_STALE_US: u64 = 2_000_000;
/// How far each air value moves to be news, in its own unit: particles in
/// µg/m³, carbon dioxide in ppm, humidity in percent, temperature in °C. A
/// value that comes or goes (known, then not) is news too, and so is `feel`.
pub const AIR_STEPS: [(&str, f64); 7] = [
    ("pm1_0", 5.0),
    ("pm2_5", 5.0),
    ("pm4_0", 5.0),
    ("pm10", 5.0),
    ("co2_ppm", 50.0),
    ("humidity", 3.0),
    ("temperature", 0.5),
];

/// An air reading as told: each value `air/state` knows (a value the sensor
/// doesn't know yet is left out), and `feel`.
fn air_reading(payload: &Value) -> Value {
    let mut reading = serde_json::Map::new();
    for (name, _) in AIR_STEPS {
        // The sensor's f32 widened is 12.300000190734863: told to a tenth,
        // and carbon dioxide whole, as the sensor measures them.
        if let Some(value) = payload[name].as_f64() {
            let told = if name == "co2_ppm" {
                json!(value.round() as i64)
            } else {
                json!((value * 10.0).round() / 10.0)
            };
            reading.insert(name.to_owned(), told);
        }
    }
    if let Some(feel) = payload["feel"].as_str() {
        reading.insert("feel".to_owned(), json!(feel));
    }
    Value::Object(reading)
}

/// Whether `now` is news over `told`: a value moved by its step, or came
/// or went, or `feel` changed.
fn air_moved(told: &Value, now: &Value) -> bool {
    told["feel"] != now["feel"]
        || AIR_STEPS.iter().any(
            |(name, step)| match (told[name].as_f64(), now[name].as_f64()) {
                (Some(a), Some(b)) => (a - b).abs() >= *step,
                (None, None) => false,
                _ => true,
            },
        )
}

/// The lamp's own computer as `get` `device` tells it, from `board/state`.
fn device_reading(payload: &Value) -> Value {
    let mut reading = serde_json::Map::new();
    for name in ["cpu_load", "memory_used", "disk_free", "uptime_s"] {
        if payload[name].is_number() {
            reading.insert(name.to_owned(), payload[name].clone());
        }
    }
    if payload["temperature_c"].is_object() {
        reading.insert("temperature_c".to_owned(), payload["temperature_c"].clone());
    }
    Value::Object(reading)
}

/// What `voice/line` means for `lamp.said`: the line's words, who had it
/// said and where it came from (never the `app`, which is a computer's id),
/// and who may be told, or nothing.
fn said_line(payload: &Value) -> Option<Out> {
    let text = payload["text"]
        .as_str()
        .filter(|text| !text.trim().is_empty())?;
    let (by, source) = (payload["by"].as_str()?, payload["source"].as_str()?);
    let to = match (by, source) {
        ("lamp", "cached") => SaidTo::Everyone,
        ("lamp", "reply") if payload["scope"] == "owner" => SaidTo::Everyone,
        ("computer", "exact") => SaidTo::Client(payload["app"].as_str()?.to_owned()),
        _ => return None,
    };
    let mut fields = json!({"text": text, "by": by, "source": source});
    if payload["cut"] == true {
        fields["cut"] = json!(true);
    }
    Some(Out::Said { fields, to })
}

/// A reading with `known: true`, or `{"known": false}` without one.
fn known(reading: Option<&(Value, u64)>) -> Value {
    match reading {
        Some((Value::Object(values), _)) => {
            let mut state = values.clone();
            state.insert("known".to_owned(), json!(true));
            Value::Object(state)
        }
        _ => json!({"known": false}),
    }
}

/// What a bus message means for the door.
#[derive(Clone, Debug, PartialEq)]
pub enum Out {
    /// An event for every connection subscribed to `kind`.
    Event(EventKind, Value),
    /// Words heard, finished: for phrase matching only, never sent.
    Heard(String),
    /// A line the lamp said, for `lamp.said` to the computers holding
    /// `speech` that `to` names (docs/control.md, "What the lamp says").
    Said { fields: Value, to: SaidTo },
    /// Every lease (on a part, or all) suspended while someone talks.
    Suspend(Option<Part>, LeaseWhy),
    /// Talking ended: suspended leases held again.
    Resume,
    /// Every lease (on a part, or all) lost.
    Lose(Option<Part>, LeaseWhy),
}

/// Who a line the lamp said goes to.
#[derive(Clone, Debug, PartialEq)]
pub enum SaidTo {
    /// Every computer holding `speech` and subscribed.
    Everyone,
    /// Only the computer (its id) that had the lamp say it.
    Client(String),
}

/// A hand in the picture, as `humans/hands/tracked` says it: the palm's
/// centre from the left and the top, the palm's width, each a share of the
/// picture's width or height, never the picture or its keypoints.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hand {
    pub x: f64,
    pub y: f64,
    pub size: f64,
    pub open: bool,
    pub confidence: f64,
}

impl Hand {
    /// One from the bus, if it is one: every share from 0 to 1, and the
    /// tracker sure enough.
    pub fn of(payload: &Value) -> Option<Self> {
        let share = |key: &str| {
            payload[key]
                .as_f64()
                .filter(|value| (0.0..=1.0).contains(value))
        };
        let hand = Self {
            x: share("x")?,
            y: share("y")?,
            size: share("size")?,
            open: payload["open"].as_bool()?,
            confidence: share("confidence")?,
        };
        (hand.confidence >= HAND_BAR).then_some(hand)
    }

    /// Whether it says anything `told` didn't.
    fn news_over(&self, told: &Self) -> bool {
        self.open != told.open
            || (self.x - told.x).abs() >= HAND_MOVED
            || (self.y - told.y).abs() >= HAND_MOVED
            || (self.size - told.size).abs() >= HAND_MOVED
    }

    fn told(&self) -> Value {
        let two = |value: f64| (value * 100.0).round() / 100.0;
        json!({"seen": true, "x": two(self.x), "y": two(self.y), "size": two(self.size), "open": self.open, "confidence": two(self.confidence)})
    }
}

/// The newest of everything the door reads.
#[derive(Debug, Default)]
pub struct Watch {
    count: usize,
    /// When the newest `humans/faces/tracked` came; none before the first.
    faces_us: Option<u64>,
    /// Whether `count` is fresh: a faces message within [`FACES_STALE_US`]
    /// ([`Self::lapse`]). Unseen until the first.
    seen: bool,
    near: f64,
    facing: bool,
    seen_us: Option<u64>,
    present: bool,
    talking: bool,
    paused: bool,
    private: bool,
    /// When the privacy lease was last heard; none before the first.
    privacy_us: Option<u64>,
    asleep: bool,
    light: Value,
    /// What the ring draws now (`light/drawn`): the person's light, or a
    /// light a computer or a skill asked for over it; none until it says.
    shows: Option<Value>,
    mood: Value,
    /// The speaker's volume (`speaker/volume`): `{level, muted}`, none until
    /// lamp-actions says it.
    volume: Option<Value>,
    /// The body's last `body/limits` and `body/joint_states`, for `get`
    /// `body`, and when each was heard: none once not heard for
    /// [`READING_STALE_US`], so a stopped or restarted body is not known
    /// and `joints` never fills a joint from an old position.
    body_limits: Option<(Value, u64)>,
    body_joints: Option<(Value, u64)>,
    sounds: [bool; 4],
    /// The newest hand heard, and when.
    hand: Option<(Hand, u64)>,
    /// The last hand told, none once its going is told, and when.
    hand_told: Option<Hand>,
    hand_told_us: Option<u64>,
    /// The newest air reading (`air/state`, its known values and `feel`),
    /// and when it was heard.
    air: Option<(Value, u64)>,
    /// The last air reading told as `air.changed`, and when.
    air_told: Option<Value>,
    air_told_us: Option<u64>,
    /// The newest reading of the lamp's own computer (`board/state`), and
    /// when it was heard.
    device: Option<(Value, u64)>,
    /// Where the body is, for `body.state`.
    body: BodyState,
    /// The head pet last told as `touch` (its controls session and contact),
    /// so one pet, said at most ten times a second while it lasts, is one
    /// event.
    pet_told: Option<(String, u64)>,
}

impl Watch {
    pub fn new() -> Self {
        Self::default()
    }

    /// What [`Watch::take`] reads from a state topic's message, for the bus
    /// thread to pass over one that says nothing new: `voice/privacy` comes
    /// a hundred times a second (its lease, `.every(10 ms)`), and each one
    /// parsed and handed to the runtime was lamp-control's idle CPU on
    /// lamp-75c9 (2% of a core). None for a topic whose every message is
    /// news (an event, or the faces, whose leaving is timed).
    pub fn kept_of(topic: &str, payload: &Value) -> Option<Value> {
        match topic {
            "voice/privacy" => Some(json!(
                payload["permitted"] == false || payload["physical_muted"] == true
            )),
            "lamp/pause" => Some(json!(payload["paused"] == true)),
            "voice/mute" => Some(json!(
                payload["muted"] == true && payload["reason"] == "sleep"
            )),
            "voice/state" => Some(json!(
                payload["state"]
                    .as_str()
                    .is_some_and(|state| state != "resting")
            )),
            "light/state" | "light/drawn" => Some(
                json!({"on": payload["on"], "colour": payload["colour"], "level": payload["level"]}),
            ),
            "mood/state" => Some(
                json!({"energy": payload["energy"], "warmth": payload["warmth"], "awake": payload["awake"], "asleep": payload["asleep"]}),
            ),
            _ => None,
        }
    }

    /// One message on `topic`, its JSON payload, at `now_us`.
    pub fn take(&mut self, topic: &str, payload: &Value, now_us: u64) -> Vec<Out> {
        match topic {
            "humans/faces/tracked" => self.faces(payload, now_us),
            "body/limits" => {
                self.body_limits = Some((payload.clone(), now_us));
                Vec::new()
            }
            "humans/hands/tracked" => match Hand::of(payload) {
                Some(hand) => {
                    self.hand = Some((hand, now_us));
                    self.hand_news(now_us)
                }
                None => Vec::new(),
            },
            // A side only when the bus gives one: the schema has no null
            // side (found by `event_schema`, #3868).
            "room/stood_up" => vec![Out::Event(
                EventKind::PersonStoodUp,
                match payload["side"].as_str() {
                    Some(side) => json!({"side": side}),
                    None => json!({}),
                },
            )],
            // With the listener's score, to a hundredth as `sound.heard`'s,
            // so a program can set its own bar: a sound like the name can
            // reach the lamp's (Queue #5385, 9 Nov 15:00 and 15:36). Left
            // out when the bus gives none.
            "name/heard" => vec![Out::Event(
                EventKind::NameHeard,
                match payload["score"].as_f64() {
                    Some(score) if score.is_finite() => {
                        json!({"score": (score.clamp(0.0, 1.0) * 100.0).round() / 100.0})
                    }
                    _ => json!({}),
                },
            )],
            "voice/heard" => match (payload["text"].as_str(), payload["finished"].as_bool()) {
                (Some(text), Some(true)) if !text.trim().is_empty() => {
                    vec![Out::Heard(text.to_owned())]
                }
                _ => Vec::new(),
            },
            // What the lamp said, gated before it goes anywhere (privacy read
            // of the `speech` design, Thirteen's P2-2): a fixed phrase to
            // every computer; a computer's own exact line back to that
            // computer alone; a reply only when the voice says it was said
            // in the owner's own scope. A reply with no scope, another
            // person's, and a skill's line are not sent: unknown is not
            // sent.
            "voice/line" => said_line(payload).into_iter().collect(),
            "ears/classes" => self.sounds(payload),
            "controls/gesture" => {
                if payload["enabled"] == false {
                    return Vec::new();
                }
                let mut out = Vec::new();
                if payload["intent"] == "stop" {
                    out.push(Out::Lose(None, LeaseWhy::StopHeard));
                }
                let kind = payload["gesture"].as_str().unwrap_or_default();
                let event = match kind {
                    "long_press" => Out::Event(EventKind::Button, json!({"press": "long_press"})),
                    // The schema's kinds are tap, pet and hold: two taps or
                    // more are a tap (the Reviewer-lead's P2 on #3868).
                    "tap" | "double_tap" | "multi_tap" => {
                        Out::Event(EventKind::Touch, json!({"kind": "tap"}))
                    }
                    "pet" => Out::Event(EventKind::Touch, json!({"kind": "pet"})),
                    _ => return out,
                };
                out.push(event);
                out
            }
            // A hand on the head, as lamp-controls says it: each contact
            // (`lamp_msgs/TouchEvent`) says `touch` when it begins, `stroke`
            // while it slides and `tap` once it is let go quickly. Only a
            // tap is told here (11091: nothing but a button's `controls/
            // gesture` ever was, so no head tap reached a program); a pet
            // is `controls/pet`'s.
            "controls/touch" => {
                if payload["enabled"] == true
                    && payload["source"] == "head"
                    && payload["gesture"] == "tap"
                    && !payload["released_us"].is_null()
                {
                    vec![Out::Event(EventKind::Touch, json!({"kind": "tap"}))]
                } else {
                    Vec::new()
                }
            }
            // A hand petting the head (`lamp_msgs/HeadPet`): said when it is
            // told apart, again while it lasts and once more as it ends;
            // the first of a contact is the event.
            "controls/pet" => {
                let (Some(session), Some(contact)) = (
                    payload["session_id"].as_str(),
                    payload["contact_id"].as_u64(),
                ) else {
                    return Vec::new();
                };
                let told = Some((session.to_owned(), contact));
                if payload["enabled"] != true || payload["ended"] == true || self.pet_told == told {
                    return Vec::new();
                }
                self.pet_told = told;
                vec![Out::Event(EventKind::Touch, json!({"kind": "pet"}))]
            }
            // Every change is said by lamp-actions, the speaker's only
            // writer: told when it differs from the one before, so the first
            // heard (the one kept on the bus) is state, not news.
            "speaker/volume" => {
                let (Some(level), Some(muted)) = (
                    payload["volume"].as_u64().filter(|level| *level <= 100),
                    payload["muted"].as_bool(),
                ) else {
                    return Vec::new();
                };
                let now = json!({"level": level, "muted": muted});
                let before = self.volume.replace(now.clone());
                match before {
                    Some(before) if before != now => {
                        vec![Out::Event(EventKind::VolumeChanged, now)]
                    }
                    _ => Vec::new(),
                }
            }
            // Kept for `get` `body`, and told as `body.state`, in degrees
            // from the homes of the body's last limits.
            "body/joint_states" => {
                self.body_joints = Some((payload.clone(), now_us));
                let lamp = json!({
                    "light": self.shows.clone().unwrap_or_else(|| self.light.clone()),
                    "talking": self.talking,
                    "private": self.private,
                });
                let limits = self.body_limits.as_ref().map(|(limits, _)| limits);
                self.body
                    .joints(payload, limits.unwrap_or(&Value::Null), lamp, now_us)
                    .map(|state| Out::Event(EventKind::BodyState, state))
                    .into_iter()
                    .collect()
            }
            "diagnostics" => {
                self.body.report(payload, now_us);
                Vec::new()
            }
            "body/hand" if payload["pushed"] == true => {
                vec![Out::Lose(Some(Part::Body), LeaseWhy::Hand)]
            }
            "voice/state" => {
                let talking = Self::kept_of(topic, payload) == Some(json!(true));
                if talking == self.talking {
                    return Vec::new();
                }
                self.talking = talking;
                let signal = if talking {
                    Out::Suspend(None, LeaseWhy::Conversation)
                } else {
                    Out::Resume
                };
                vec![signal, self.lamp_changed()]
            }
            "lamp/pause" => self.flag(
                Self::kept_of(topic, payload) == Some(json!(true)),
                |watch| &mut watch.paused,
                LeaseWhy::Paused,
            ),
            "voice/mute" => {
                let asleep = Self::kept_of(topic, payload) == Some(json!(true));
                self.flag(asleep, |watch| &mut watch.asleep, LeaseWhy::Asleep)
            }
            "voice/privacy" => {
                self.privacy_us = Some(now_us);
                let private = Self::kept_of(topic, payload) == Some(json!(true));
                self.flag(private, |watch| &mut watch.private, LeaseWhy::Private)
            }
            "light/state" => {
                self.light = Self::kept_of(topic, payload).unwrap_or_default();
                Vec::new()
            }
            "light/drawn" => {
                self.shows = Self::kept_of(topic, payload);
                Vec::new()
            }
            "mood/state" => {
                self.mood = Self::kept_of(topic, payload).unwrap_or_default();
                Vec::new()
            }
            "air/state" => {
                self.air = Some((air_reading(payload), now_us));
                self.air_news(now_us)
            }
            "board/state" => {
                self.device = Some((device_reading(payload), now_us));
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// Whether the bus thread may pass over a state message whose key is
    /// `key`: the same as the one it last handed on (`sent`, with when) and
    /// handed on under [`REPEAT_EVERY_US`] ago.
    pub fn passes_over(sent: Option<&(Value, u64)>, key: &Value, now_us: u64) -> bool {
        sent.is_some_and(|(last, at)| last == key && now_us.saturating_sub(*at) < REPEAT_EVERY_US)
    }

    /// Time passing with no message: a privacy lease last handed on
    /// [`PRIVACY_LAPSE_US`] ago or more is private, later than the voice's
    /// own microphone gate closes (harness-reviewer's P3 on #4031). Before the
    /// first lease nothing lapses: a lamp whose voice doesn't publish one
    /// isn't private forever.
    ///
    /// A hand not heard of for [`HAND_GONE_US`] is gone, and one held back
    /// by [`HAND_EVERY_US`] is told now.
    pub fn lapse(&mut self, now_us: u64) -> Vec<Out> {
        let stale = |reading: &Option<(Value, u64)>| {
            reading
                .as_ref()
                .is_some_and(|(_, heard)| now_us.saturating_sub(*heard) >= READING_STALE_US)
        };
        if stale(&self.air) {
            self.air = None;
        }
        if stale(&self.device) {
            self.device = None;
        }
        if stale(&self.body_limits) {
            self.body_limits = None;
        }
        if stale(&self.body_joints) {
            self.body_joints = None;
        }
        if self
            .faces_us
            .is_none_or(|heard| now_us.saturating_sub(heard) >= FACES_STALE_US)
        {
            self.seen = false;
        }
        let mut out = self.hand_news(now_us);
        out.extend(self.air_news(now_us));
        let lapsed = self
            .privacy_us
            .is_some_and(|heard| now_us.saturating_sub(heard) >= PRIVACY_LAPSE_US);
        if lapsed && !self.private {
            out.extend(self.flag(true, |watch| &mut watch.private, LeaseWhy::Private));
        }
        out
    }

    /// The hand, by edge: seen after none, moved or grown by [`HAND_MOVED`],
    /// opened or closed, at most every [`HAND_EVERY_US`] with the newest
    /// held back until then; and gone, once.
    fn hand_news(&mut self, now_us: u64) -> Vec<Out> {
        let Some((hand, heard_us)) = self.hand else {
            return Vec::new();
        };
        if now_us.saturating_sub(heard_us) >= HAND_GONE_US {
            self.hand = None;
            if self.hand_told.take().is_none() {
                return Vec::new();
            }
            self.hand_told_us = Some(now_us);
            return vec![Out::Event(EventKind::Hand, json!({"seen": false}))];
        }
        let news = self.hand_told.is_none_or(|told| hand.news_over(&told));
        let due = self
            .hand_told_us
            .is_none_or(|told| now_us.saturating_sub(told) >= HAND_EVERY_US);
        if !news || !due {
            return Vec::new();
        }
        self.hand_told = Some(hand);
        self.hand_told_us = Some(now_us);
        vec![Out::Event(EventKind::Hand, hand.told())]
    }

    /// The air, by edge: the first reading, then one where a value moved by
    /// its [`AIR_STEPS`], came or went, or `feel` changed, at most every
    /// [`AIR_EVERY_US`] with the newest held back until then.
    fn air_news(&mut self, now_us: u64) -> Vec<Out> {
        let Some((air, _)) = &self.air else {
            return Vec::new();
        };
        let news = self
            .air_told
            .as_ref()
            .is_none_or(|told| air_moved(told, air));
        let due = self
            .air_told_us
            .is_none_or(|told| now_us.saturating_sub(told) >= AIR_EVERY_US);
        if !news || !due {
            return Vec::new();
        }
        let air = air.clone();
        self.air_told = Some(air.clone());
        self.air_told_us = Some(now_us);
        vec![Out::Event(EventKind::AirChanged, air)]
    }

    /// The state `get` answers for `kind`.
    /// Whether the lamp is private now: the privacy switch, or a privacy
    /// lease that stopped coming ([`Self::lapse`]).
    pub fn private(&self) -> bool {
        self.private
    }

    /// How many faces are in view now, by the newest `humans/faces/tracked`:
    /// counts, never who. None while the room is unseen: no faces message
    /// for [`FACES_STALE_US`] ([`Self::lapse`]), or none yet.
    pub fn room(&self) -> Option<usize> {
        self.seen.then_some(self.count)
    }

    pub fn state(&self, kind: crate::protocol::StateKind) -> Value {
        use crate::protocol::StateKind;
        match kind {
            StateKind::Presence => {
                json!({"count": self.count, "near": self.near, "facing": self.facing})
            }
            // The person's own light as before, and what shows beside it.
            StateKind::Light => match (&self.light, &self.shows) {
                (light, None) => light.clone(),
                (light, Some(shows)) => {
                    let mut state = light.as_object().cloned().unwrap_or_default();
                    state.insert("shows".to_owned(), shows.clone());
                    Value::Object(state)
                }
            },
            StateKind::Mood => self.mood.clone(),
            // The newest reading, with `known`; `known: false` with no sensor,
            // its part not running, or nothing in [`READING_STALE_US`].
            StateKind::Air => known(self.air.as_ref()),
            StateKind::Device => known(self.device.as_ref()),
            StateKind::Volume => self.volume.clone().unwrap_or(Value::Null),
            StateKind::Body => {
                let heard = |reading: &Option<(Value, u64)>| {
                    reading
                        .as_ref()
                        .map_or(Value::Null, |(value, _)| value.clone())
                };
                crate::body_info::state(&heard(&self.body_limits), &heard(&self.body_joints))
            }
            StateKind::Lamp => {
                json!({"talking": self.talking, "paused": self.paused, "private": self.private, "asleep": self.asleep})
            }
        }
    }

    fn lamp_changed(&self) -> Out {
        Out::Event(
            EventKind::LampChanged,
            // Inside `lamp_state`, as control.schema.json's event says
            // (the Reviewer-lead's P2 on #3868).
            json!({"lamp_state": {"talking": self.talking, "paused": self.paused, "private": self.private, "asleep": self.asleep}}),
        )
    }

    /// A yes-or-no of the lamp's own: on its rising edge every lease is lost.
    fn flag(
        &mut self,
        now: bool,
        field: impl Fn(&mut Self) -> &mut bool,
        why: LeaseWhy,
    ) -> Vec<Out> {
        let was = std::mem::replace(field(self), now);
        if was == now {
            return Vec::new();
        }
        let mut out = Vec::new();
        if now {
            out.push(Out::Lose(None, why));
        }
        out.push(self.lamp_changed());
        out
    }

    fn faces(&mut self, payload: &Value, now_us: u64) -> Vec<Out> {
        let Some(faces) = payload["faces"].as_array() else {
            return Vec::new();
        };
        self.count = faces.len();
        self.faces_us = Some(now_us);
        self.seen = true;
        // To two places, as a sound's score is (harness-reviewer's P3 on
        // #3868: 0.39999999999999997).
        self.near = (faces
            .iter()
            .filter_map(|face| Some(face["ymax"].as_f64()? - face["ymin"].as_f64()?))
            .fold(0.0, f64::max)
            * 100.0)
            .round()
            / 100.0;
        let mut out = Vec::new();
        let facing = faces.iter().any(|face| face["facing"] == true);
        if !faces.is_empty() {
            let away = self
                .seen_us
                .is_none_or(|seen| now_us.saturating_sub(seen) >= ABSENT_FOR_US);
            if !self.present && away {
                out.push(Out::Event(
                    EventKind::PersonArrived,
                    json!({"count": self.count, "near": self.near}),
                ));
            }
            self.present = true;
            self.seen_us = Some(now_us);
        } else if self.present
            && self
                .seen_us
                .is_some_and(|seen| now_us.saturating_sub(seen) >= LEFT_AFTER_US)
        {
            self.present = false;
            // Nobody in view now: the schema's `count` (the Reviewer-lead's
            // P2 on #3868).
            out.push(Out::Event(EventKind::PersonLeft, json!({"count": 0})));
        }
        if facing != self.facing {
            self.facing = facing;
            out.push(Out::Event(
                EventKind::PersonFacing,
                json!({"facing": facing}),
            ));
        }
        out
    }

    fn sounds(&mut self, payload: &Value) -> Vec<Out> {
        let scores = &payload["scores"];
        let mut out = Vec::new();
        for (index, class) in SOUND_CLASSES.iter().enumerate() {
            let score = scores[*class].as_f64().unwrap_or(0.0);
            let heard = score >= SOUND_BAR;
            if heard && !self.sounds[index] {
                out.push(Out::Event(
                    EventKind::SoundHeard,
                    json!({"class": class, "score": (score * 100.0).round() / 100.0}),
                ));
            }
            self.sounds[index] = heard;
        }
        out
    }
}

/// The phrases of `phrases` that `heard` holds, whole words, ignoring case
/// and punctuation: "Lights please!" holds "lights please".
pub fn phrases_in<'a>(heard: &str, phrases: &'a [String]) -> Vec<&'a str> {
    // Each word without its apostrophes, so "it's" and "its" are one.
    let words = |text: &str| -> Vec<String> {
        crate::protocol::phrase_words(text)
            .into_iter()
            .map(|word| word.replace('\'', ""))
            .filter(|word| !word.is_empty())
            .collect()
    };
    let heard = words(heard);
    phrases
        .iter()
        .filter(|phrase| {
            let wanted: String = words(phrase).concat();
            !wanted.is_empty() && spells(&heard, &wanted)
        })
        .map(String::as_str)
        .collect()
}

/// Whether some run of whole `heard` words spells `wanted`, its words'
/// letters joined: the recognizer may write "goodnight" where the phrase
/// says "good night", or the other way round (lamp-75c9, 9 Nov: a line
/// heard with the phrase in it, and nothing told). The heard run may also
/// end in one "s" more than the phrase: the recognizer wrote "lamps" for a
/// played "lamp" (lamp-75c9 run13b, 10 Nov). Only the heard side gains it,
/// so a phrase is matched as written ("please press s" by itself, "yes"
/// never by "ye"). Each run is compared at every word edge until it is
/// longer than the phrase and one "s", so a short prefix never ends the
/// search; "goodnights lamp" or "good knight lamp" never match.
fn spells(heard: &[String], wanted: &str) -> bool {
    (0..heard.len()).any(|start| {
        let mut joined = String::new();
        for word in &heard[start..] {
            joined.push_str(word);
            if joined == wanted || joined.strip_suffix('s') == Some(wanted) {
                return true;
            }
            if joined.len() > wanted.len() {
                return false;
            }
        }
        false
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::StateKind;

    /// lamp-75c9, 9 Nov: the recognizer's own spelling still matches.
    /// Words are joined over a run of whole words, both ways round; a
    /// different word, or a longer one, never matches.
    #[test]
    fn a_phrase_matches_however_its_words_are_joined() {
        let phrase = vec!["good night lamp".to_owned()];
        assert_eq!(
            phrases_in("Lamp. Goodnight, lamp.", &phrase),
            ["good night lamp"]
        );
        assert_eq!(
            phrases_in("Lamp. Good night lamp.", &phrase),
            ["good night lamp"]
        );
        assert_eq!(phrases_in("good-night lamp", &phrase), ["good night lamp"]);
        let joined = vec!["goodnight lamp".to_owned()];
        assert_eq!(phrases_in("Good night, lamp!", &joined), ["goodnight lamp"]);
        assert!(phrases_in("good knight lamp", &phrase).is_empty());
        assert!(phrases_in("goodnights lamp", &phrase).is_empty());
        assert_eq!(
            phrases_in("so good nightlamp", &phrase),
            ["good night lamp"]
        );
        let its = vec!["it's bedtime".to_owned()];
        assert_eq!(phrases_in("Its bedtime now", &its), ["it's bedtime"]);
        assert!(phrases_in("night lamp", &phrase).is_empty());
    }

    /// lamp-75c9 run13b, 10 Nov: the recognizer wrote "lamps" for a
    /// played "good night lamp", and only the phrases without "lamp" were
    /// told. The heard run may end in one "s" more than the phrase, at its
    /// end only; a phrase is always matched as written (reviewer-two's P2 on
    /// #5971: "please press s" stopped matching itself).
    #[test]
    fn a_phrase_matches_its_last_word_with_an_s_more() {
        let phrase = vec!["good night lamp".to_owned()];
        assert_eq!(
            phrases_in("Lamp good night lamps", &phrase),
            ["good night lamp"]
        );
        assert_eq!(
            phrases_in("good night lamp's", &phrase),
            ["good night lamp"]
        );
        let light = vec!["turn off the light".to_owned()];
        assert_eq!(
            phrases_in("Please turn off the lights now.", &light),
            ["turn off the light"]
        );
        // Only the run's end: an "s" inside it is another word.
        assert!(phrases_in("goodnights lamp", &phrase).is_empty());
        assert!(phrases_in("good nights lamp", &phrase).is_empty());
        // One "s", no more, and never another letter.
        assert!(phrases_in("good night lampss", &phrase).is_empty());
        assert!(phrases_in("good night lamb", &phrase).is_empty());
        assert!(phrases_in("good night lampshade", &phrase).is_empty());
        // The phrase side never loses its "s".
        let lights = vec!["turn off the lights".to_owned()];
        assert!(phrases_in("turn off the light", &lights).is_empty());
        let yes = vec!["yes please lamps".to_owned()];
        assert!(phrases_in("yes please lamp", &yes).is_empty());
    }

    /// Reviewer-two's P2 on #5971: a phrase whose last word ends in "s",
    /// or is "s", matches identical heard words; a run is not given up at
    /// the first word edge that reaches the phrase without its "s".
    #[test]
    fn a_phrase_ending_in_s_matches_itself() {
        for phrase in [
            "please press s",
            "please pass s",
            "turn off the lights",
            "good night lamps",
        ] {
            let phrases = vec![phrase.to_owned()];
            assert_eq!(phrases_in(phrase, &phrases), [phrase], "{phrase}");
            assert_eq!(
                phrases_in(&format!("and {phrase} now"), &phrases),
                [phrase],
                "{phrase} inside a line"
            );
        }
        let press = vec!["please press s".to_owned()];
        assert_eq!(phrases_in("please presss", &press), ["please press s"]);
        assert!(phrases_in("please press", &press).is_empty());
    }

    const S: u64 = 1_000_000;

    fn face(ymin: f64, ymax: f64, facing: bool) -> Value {
        json!({"id": "f1", "xmin": 0.4, "ymin": ymin, "xmax": 0.6, "ymax": ymax, "confidence": 0.9, "facing": facing})
    }

    fn kinds(out: &[Out]) -> Vec<EventKind> {
        out.iter()
            .filter_map(|out| match out {
                Out::Event(kind, _) => Some(*kind),
                _ => None,
            })
            .collect()
    }

    /// A voice that stops renewing its privacy lease leaves the lamp private
    /// once [`PRIVACY_LAPSE_US`] passes, and a lease heard again opens it.
    #[test]
    fn a_privacy_lease_that_stops_coming_is_private() {
        let open = json!({"permitted": true, "physical_muted": false});
        let mut watch = Watch::new();
        assert!(
            watch.lapse(10 * S).is_empty(),
            "no lease yet: nothing lapses"
        );
        assert!(watch.take("voice/privacy", &open, 10 * S).is_empty());
        assert!(watch.lapse(10 * S + PRIVACY_LAPSE_US - 1).is_empty());
        let out = watch.lapse(10 * S + PRIVACY_LAPSE_US);
        assert_eq!(out[0], Out::Lose(None, LeaseWhy::Private));
        assert_eq!(watch.state(StateKind::Lamp)["private"], true);
        assert!(watch.lapse(20 * S).is_empty(), "lost once, not every tick");
        assert_eq!(
            kinds(&watch.take("voice/privacy", &open, 20 * S)),
            [EventKind::LampChanged]
        );
        assert_eq!(watch.state(StateKind::Lamp)["private"], false);
    }

    /// The local leader's P2 on the microphone: the face count is only as
    /// good as its last message. Unseen before the first, its count while
    /// fresh, and unseen again once none has come for [`FACES_STALE_US`],
    /// whatever it last said.
    #[test]
    fn the_room_is_unseen_once_the_face_count_goes_stale() {
        let mut watch = Watch::new();
        assert_eq!(watch.room(), None, "unseen before the first message");
        watch.lapse(S);
        assert_eq!(watch.room(), None);
        let face = json!({"ymin": 0.2, "ymax": 0.5});
        watch.take("humans/faces/tracked", &json!({"faces": [face]}), 2 * S);
        assert_eq!(watch.room(), Some(1));
        watch.lapse(2 * S + FACES_STALE_US - 1);
        assert_eq!(watch.room(), Some(1), "still fresh");
        watch.lapse(2 * S + FACES_STALE_US);
        assert_eq!(watch.room(), None, "a frozen 1 is unknown");
        let both = json!({"faces": [face, face]});
        watch.take("humans/faces/tracked", &both, 5 * S);
        assert_eq!(watch.room(), Some(2));
        watch.take("humans/faces/tracked", &json!({"faces": []}), 6 * S);
        assert_eq!(watch.room(), Some(0), "nobody, seen");
    }

    /// A hand is told by edge and at most every [`HAND_EVERY_US`], the
    /// newest held back until then, and gone once after [`HAND_GONE_US`]
    /// with no message: the topic sends none without a hand.
    #[test]
    fn a_hand_is_told_by_edge_rate_limited_and_gone_after_silence() {
        let hand = |x: f64, open: bool| json!({"x": x, "y": 0.5, "size": 0.2, "open": open, "confidence": 0.9, "t": 1});
        let told = |out: &[Out]| match out {
            [Out::Event(EventKind::Hand, value)] => value.clone(),
            other => panic!("one hand event, not {other:?}"),
        };
        let mut watch = Watch::new();
        let topic = "humans/hands/tracked";
        assert_eq!(
            told(&watch.take(topic, &hand(0.3, true), S)),
            json!({"seen": true, "x": 0.3, "y": 0.5, "size": 0.2, "open": true, "confidence": 0.9})
        );
        // Still, or nearly: nothing, at camera rate.
        assert!(watch.take(topic, &hand(0.31, true), S + 33_000).is_empty());
        // Moved, but too soon: held back, and told at the next check.
        assert!(watch.take(topic, &hand(0.5, true), S + 66_000).is_empty());
        assert!(watch.lapse(S + HAND_EVERY_US - 1).is_empty());
        assert_eq!(told(&watch.lapse(S + HAND_EVERY_US))["x"], 0.5);
        assert!(watch.lapse(S + 2 * HAND_EVERY_US).is_empty(), "told once");
        // Closed: news.
        let closed = told(&watch.take(topic, &hand(0.5, false), 2 * S));
        assert_eq!(closed["open"], false);
        // Not a hand: out of the picture, unsure, or no `open`.
        for payload in [
            json!({"x": 1.5, "y": 0.5, "size": 0.2, "open": true, "confidence": 0.9}),
            json!({"x": 0.9, "y": 0.5, "size": 0.2, "open": true, "confidence": 0.2}),
            json!({"x": 0.9, "y": 0.5, "size": 0.2, "confidence": 0.9}),
        ] {
            assert!(watch.take(topic, &payload, 3 * S).is_empty(), "{payload}");
        }
        // Silence: gone after HAND_GONE_US from the last message, once.
        assert!(watch.lapse(2 * S + HAND_GONE_US - 1).is_empty());
        assert_eq!(
            told(&watch.lapse(2 * S + HAND_GONE_US)),
            json!({"seen": false})
        );
        assert!(watch.lapse(10 * S).is_empty());
        assert_eq!(
            told(&watch.take(topic, &hand(0.5, false), 11 * S))["seen"],
            true
        );
        assert_eq!(EventKind::Hand.needs(), Some(crate::protocol::Grant::Look));
    }

    /// The bus thread hands on a repeat at least every [`REPEAT_EVERY_US`],
    /// so a renewed lease never looks lapsed; a new key goes on at once.
    #[test]
    fn a_repeated_state_message_is_handed_on_now_and_then() {
        let key = json!(false);
        let sent = (key.clone(), 5 * S);
        assert!(!Watch::passes_over(None, &key, 5 * S));
        assert!(Watch::passes_over(
            Some(&sent),
            &key,
            5 * S + REPEAT_EVERY_US - 1
        ));
        assert!(!Watch::passes_over(
            Some(&sent),
            &key,
            5 * S + REPEAT_EVERY_US
        ));
        assert!(!Watch::passes_over(Some(&sent), &json!(true), 5 * S + 1));
        const { assert!(REPEAT_EVERY_US * 2 < PRIVACY_LAPSE_US) };
    }

    /// A state topic's message whose [`Watch::kept_of`] is the last one's
    /// changes nothing in [`Watch::take`], so the bus thread may pass it
    /// over; one whose key differs is news. Event topics have no key.
    #[test]
    fn a_state_message_with_the_same_key_changes_nothing() {
        let pairs = [
            (
                "voice/privacy",
                json!({"permitted": true, "physical_muted": false, "stamp": {"sequence": 1}}),
                json!({"permitted": true, "physical_muted": false, "stamp": {"sequence": 2}}),
                json!({"permitted": false, "stamp": {"sequence": 3}}),
            ),
            (
                "lamp/pause",
                json!({"paused": false, "at": 1}),
                json!({"paused": false, "at": 2}),
                json!({"paused": true}),
            ),
            (
                "voice/mute",
                json!({"muted": true, "reason": "button"}),
                json!({"muted": false, "reason": "sleep"}),
                json!({"muted": true, "reason": "sleep"}),
            ),
            (
                "voice/state",
                json!({"state": "listening"}),
                json!({"state": "speaking"}),
                json!({"state": "resting"}),
            ),
            (
                "light/state",
                json!({"on": true, "colour": "warm", "level": 3, "at": 1}),
                json!({"on": true, "colour": "warm", "level": 3, "at": 2}),
                json!({"on": true, "colour": "warm", "level": 2}),
            ),
            (
                "light/drawn",
                json!({"on": true, "colour": "red", "level": 3, "at": 1}),
                json!({"on": true, "colour": "red", "level": 3, "at": 2}),
                json!({"on": true, "colour": "warm", "level": 1}),
            ),
            (
                "mood/state",
                json!({"energy": 0.5, "warmth": 0.4, "awake": true, "asleep": false, "at": 1}),
                json!({"energy": 0.5, "warmth": 0.4, "awake": true, "asleep": false, "at": 2}),
                json!({"energy": 0.6, "warmth": 0.4, "awake": true, "asleep": false}),
            ),
        ];
        for (topic, first, same, other) in pairs {
            let mut watch = Watch::new();
            watch.take(topic, &first, S);
            assert_eq!(
                Watch::kept_of(topic, &first),
                Watch::kept_of(topic, &same),
                "{topic}"
            );
            let state = |watch: &Watch| {
                (
                    watch.private,
                    watch.paused,
                    watch.asleep,
                    watch.talking,
                    watch.light.clone(),
                    watch.mood.clone(),
                )
            };
            let before = state(&watch);
            assert!(watch.take(topic, &same, 2 * S).is_empty(), "{topic}");
            assert_eq!(state(&watch), before, "{topic}");
            assert_ne!(
                Watch::kept_of(topic, &first),
                Watch::kept_of(topic, &other),
                "{topic}"
            );
        }
        for topic in [
            "humans/faces/tracked",
            "humans/hands/tracked",
            "voice/heard",
            "ears/classes",
            "body/hand",
            "controls/gesture",
            "name/heard",
            "room/stood_up",
            "controls/touch",
            "controls/pet",
        ] {
            assert_eq!(Watch::kept_of(topic, &json!({})), None, "{topic}");
        }
    }

    #[test]
    fn arrivals_and_leavings_are_edges_with_a_gap_never_every_frame() {
        let mut watch = Watch::new();
        let one = json!({"faces": [face(0.2, 0.5, false)]});
        let none = json!({"faces": []});
        assert_eq!(
            kinds(&watch.take("humans/faces/tracked", &one, S)),
            [EventKind::PersonArrived]
        );
        assert!(watch.take("humans/faces/tracked", &one, 2 * S).is_empty());
        // A dropout shorter than the leaving time is nobody leaving.
        assert!(watch.take("humans/faces/tracked", &none, 4 * S).is_empty());
        assert!(watch.take("humans/faces/tracked", &one, 5 * S).is_empty());
        assert_eq!(
            kinds(&watch.take("humans/faces/tracked", &none, 11 * S)),
            [EventKind::PersonLeft]
        );
        // Back at once after leaving is no new arrival until away long enough.
        assert_eq!(
            kinds(&watch.take("humans/faces/tracked", &one, 12 * S)),
            [EventKind::PersonArrived]
        );
        let turned = json!({"faces": [face(0.2, 0.5, true)]});
        assert_eq!(
            kinds(&watch.take("humans/faces/tracked", &turned, 13 * S)),
            [EventKind::PersonFacing]
        );
        assert_eq!(
            watch.state(StateKind::Presence),
            json!({"count": 1, "near": 0.3, "facing": true})
        );
    }

    #[test]
    fn talking_suspends_leases_and_the_lamps_own_flags_lose_them() {
        let mut watch = Watch::new();
        let out = watch.take("voice/state", &json!({"state": "listening"}), S);
        assert_eq!(out[0], Out::Suspend(None, LeaseWhy::Conversation));
        assert!(
            watch
                .take("voice/state", &json!({"state": "speaking"}), S)
                .is_empty()
        );
        assert_eq!(
            watch.take("voice/state", &json!({"state": "resting"}), S)[0],
            Out::Resume
        );
        assert_eq!(
            watch.take("body/hand", &json!({"pushed": true, "since_us": 1}), S),
            [Out::Lose(Some(Part::Body), LeaseWhy::Hand)]
        );
        assert!(
            watch
                .take("body/hand", &json!({"pushed": false, "since_us": 1}), S)
                .is_empty()
        );
        assert_eq!(
            watch.take("lamp/pause", &json!({"paused": true}), S)[0],
            Out::Lose(None, LeaseWhy::Paused)
        );
        assert_eq!(
            kinds(&watch.take("lamp/pause", &json!({"paused": false}), S)),
            [EventKind::LampChanged]
        );
        assert_eq!(
            watch.take("voice/privacy", &json!({"permitted": false}), S)[0],
            Out::Lose(None, LeaseWhy::Private)
        );
        // The privacy topic comes every 10 ms: only its edge counts.
        assert!(
            watch
                .take("voice/privacy", &json!({"permitted": false}), S)
                .is_empty()
        );
        assert_eq!(
            watch.take("voice/mute", &json!({"muted": true, "reason": "sleep"}), S)[0],
            Out::Lose(None, LeaseWhy::Asleep)
        );
        assert!(
            watch
                .take(
                    "voice/mute",
                    &json!({"muted": true, "reason": "software"}),
                    S
                )
                .iter()
                .all(|out| !matches!(out, Out::Lose(..)))
        );
        assert_eq!(
            watch.state(StateKind::Lamp),
            json!({"talking": false, "paused": false, "private": true, "asleep": false})
        );
        let stop = json!({"enabled": true, "gesture": "tap", "intent": "stop"});
        let out = watch.take("controls/gesture", &stop, S);
        assert_eq!(out[0], Out::Lose(None, LeaseWhy::StopHeard));
        assert_eq!(kinds(&out), [EventKind::Touch]);
        assert!(
            watch
                .take(
                    "controls/gesture",
                    &json!({"enabled": false, "gesture": "tap", "intent": "stop"}),
                    S
                )
                .is_empty()
        );
    }

    /// What lamp-controls says on `controls/touch` for a head contact, as
    /// lamp-75c9 (11091) published it while a person tapped and stroked
    /// (Ten's touch check, 10 Nov 2026): the whole `lamp_msgs/TouchEvent`.
    fn head_touch(gesture: &str, contact: u64, released: Option<u64>) -> Value {
        json!({"active_mask": 0, "active_names": [], "began_us": 417374608818u64,
            "contact_id": contact, "distance": 0.0, "enabled": true, "gesture": gesture,
            "name": "head", "observed_us": 417374911590u64, "position": null,
            "reader_session_id": "touch-gpio-584969-364420275115", "released_us": released,
            "session_id": "b90b26cdeb027a54", "source": "head", "speed": 0.0,
            "stamp": {"at_us": 417374911639u64, "clock_id": "b05003f2", "sequence": 47},
            "toward_pad": null, "when_asleep": null, "when_quiet": "attend",
            "when_speaking": "attend"})
    }

    #[test]
    fn a_head_tap_on_controls_touch_is_a_touch_tap_and_a_contact_still_down_is_not() {
        let mut watch = Watch::new();
        let touching = head_touch("touch", 23, None);
        assert!(watch.take("controls/touch", &touching, S).is_empty());
        assert!(
            watch
                .take("controls/touch", &head_touch("stroke", 23, None), S)
                .is_empty()
        );
        let out = watch.take(
            "controls/touch",
            &head_touch("tap", 24, Some(417374761802)),
            S,
        );
        assert_eq!(kinds(&out), [EventKind::Touch]);
        assert_eq!(out[0], Out::Event(EventKind::Touch, json!({"kind": "tap"})));
        // A shadow (not enabled) reading and another's source say nothing.
        let mut shadow = head_touch("tap", 25, Some(1));
        shadow["enabled"] = json!(false);
        assert!(watch.take("controls/touch", &shadow, 2 * S).is_empty());
        let mut strip = head_touch("tap", 26, Some(1));
        strip["source"] = json!("strip");
        assert!(watch.take("controls/touch", &strip, 2 * S).is_empty());
    }

    #[test]
    fn a_head_pet_on_controls_pet_is_one_touch_pet_for_the_whole_contact() {
        let mut watch = Watch::new();
        let pet = |contact: u64, swaps: u32, ended: bool| {
            json!({"stamp": {"at_us": 1, "clock_id": "c", "sequence": 3}, "session_id": "s1",
                "reader_session_id": "r", "enabled": true, "contact_id": contact,
                "began_us": 1, "petted_us": 2, "observed_us": 3, "swaps": swaps,
                "swaps_per_second": 4.0, "resting": false, "ended": ended})
        };
        let first = watch.take("controls/pet", &pet(30, 3, false), S);
        assert_eq!(
            first,
            vec![Out::Event(EventKind::Touch, json!({"kind": "pet"}))]
        );
        // Told again while it lasts and at its end: not news.
        assert!(
            watch
                .take("controls/pet", &pet(30, 5, false), 2 * S)
                .is_empty()
        );
        assert!(
            watch
                .take("controls/pet", &pet(30, 5, true), 3 * S)
                .is_empty()
        );
        // The next pet, its own contact, is.
        assert_eq!(
            kinds(&watch.take("controls/pet", &pet(31, 3, false), 6 * S)),
            [EventKind::Touch]
        );
        let mut shadow = pet(32, 3, false);
        shadow["enabled"] = json!(false);
        assert!(watch.take("controls/pet", &shadow, 9 * S).is_empty());
    }

    /// Thirteen's P2-2 on the `speech` design: a line the lamp said goes
    /// only where the privacy read allows, and unknown is not sent.
    #[test]
    fn a_line_the_lamp_said_goes_only_where_it_may() {
        let mut watch = Watch::new();
        let mut said = |payload: Value| watch.take("voice/line", &payload, S);
        // A fixed phrase: to everyone who reads what the lamp says.
        assert_eq!(
            said(json!({"text": "Going to sleep now.", "by": "lamp", "source": "cached"})),
            [Out::Said {
                fields: json!({"text": "Going to sleep now.", "by": "lamp", "source": "cached"}),
                to: SaidTo::Everyone
            }]
        );
        // A reply in the owner's own scope, cut: told as cut.
        assert_eq!(
            said(
                json!({"text": "It is", "by": "lamp", "source": "reply", "scope": "owner", "cut": true, "request": 4})
            ),
            [Out::Said {
                fields: json!({"text": "It is", "by": "lamp", "source": "reply", "cut": true}),
                to: SaidTo::Everyone
            }]
        );
        // A reply with no scope, or any other scope, is not sent.
        assert!(said(json!({"text": "Four.", "by": "lamp", "source": "reply"})).is_empty());
        assert!(
            said(json!({"text": "Four.", "by": "lamp", "source": "reply", "scope": "room"}))
                .is_empty()
        );
        assert!(
            said(json!({"text": "Four.", "by": "lamp", "source": "reply", "scope": "other"}))
                .is_empty()
        );
        // A computer's own line goes back to that computer alone, and its
        // name (an id) is not in what is sent.
        assert_eq!(
            said(
                json!({"text": "Build passed.", "by": "computer", "source": "exact", "app": "c_1"})
            ),
            [Out::Said {
                fields: json!({"text": "Build passed.", "by": "computer", "source": "exact"}),
                to: SaidTo::Client("c_1".into())
            }]
        );
        // One that names nobody, a skill's line, a recording and an empty
        // line are not sent.
        assert!(said(json!({"text": "Hi.", "by": "computer", "source": "exact"})).is_empty());
        assert!(
            said(json!({"text": "Water.", "by": "skill", "source": "exact", "app": "reminder"}))
                .is_empty()
        );
        assert!(said(json!({"text": "x", "by": "lamp", "source": "recording"})).is_empty());
        assert!(said(json!({"text": " ", "by": "lamp", "source": "cached"})).is_empty());
    }

    #[test]
    fn words_never_leave_only_phrases_match_and_sounds_rise_past_the_bar() {
        let mut watch = Watch::new();
        assert_eq!(
            watch.take(
                "voice/heard",
                &json!({"text": "Lights please!", "finished": true}),
                S
            ),
            [Out::Heard("Lights please!".into())]
        );
        assert!(
            watch
                .take(
                    "voice/heard",
                    &json!({"text": "Lights", "finished": false}),
                    S
                )
                .is_empty()
        );
        let phrases = vec!["lights please".to_owned(), "good night lamp".to_owned()];
        assert_eq!(phrases_in("Lights please!", &phrases), ["lights please"]);
        assert_eq!(
            phrases_in("lamp, good night lamp", &phrases),
            ["good night lamp"]
        );
        assert!(phrases_in("the lights pleased him", &phrases).is_empty());
        let music = json!({"scores": {"speech": 0.9, "music": 0.8, "laughter": 0.1, "door": 0.0, "footsteps": 0.0}});
        let out = watch.take("ears/classes", &music, S);
        assert_eq!(
            out,
            [Out::Event(
                EventKind::SoundHeard,
                json!({"class": "music", "score": 0.8})
            )]
        );
        assert!(
            watch.take("ears/classes", &music, S).is_empty(),
            "held, not again"
        );
        watch.take("ears/classes", &json!({"scores": {"music": 0.1}}), S);
        assert_eq!(
            kinds(&watch.take("ears/classes", &music, S)),
            [EventKind::SoundHeard]
        );
        watch.take(
            "light/state",
            &json!({"on": true, "colour": "warm", "level": 3}),
            S,
        );
        assert_eq!(
            watch.state(StateKind::Light),
            json!({"on": true, "colour": "warm", "level": 3})
        );
    }

    /// `get`'s light: the person's own as before, and what the ring draws
    /// as `shows` (lamp-a0ae, 29 Oct: light/state warm 1 while light/drawn
    /// was a computer's red 3).
    #[test]
    fn the_light_state_is_the_persons_own_and_what_shows() {
        let mut watch = Watch::new();
        assert_eq!(watch.state(StateKind::Light), Value::Null, "nothing heard");
        let red = json!({"on": true, "colour": "red", "level": 3, "source": "ring"});
        watch.take("light/drawn", &red, S);
        assert_eq!(
            watch.state(StateKind::Light),
            json!({"shows": {"on": true, "colour": "red", "level": 3}})
        );
        watch.take(
            "light/state",
            &json!({"on": true, "colour": "warm", "level": 1}),
            S,
        );
        assert_eq!(
            watch.state(StateKind::Light),
            json!({"on": true, "colour": "warm", "level": 1, "shows": {"on": true, "colour": "red", "level": 3}})
        );
        // A ring that says nothing more leaves its last draw: `shows`
        // doesn't lapse with light/drawn's 3 s (control.md says so).
        watch.lapse(60 * S);
        assert_eq!(
            watch.state(StateKind::Light)["shows"],
            json!({"on": true, "colour": "red", "level": 3})
        );
        // The request lapses: the ring draws the person's light again.
        watch.take(
            "light/drawn",
            &json!({"on": true, "colour": "warm", "level": 1}),
            2 * S,
        );
        assert_eq!(
            watch.state(StateKind::Light)["shows"],
            json!({"on": true, "colour": "warm", "level": 1})
        );
    }

    fn air(pm2_5: f32, co2: u16, feel: &str) -> Value {
        json!({"pm1_0": 3.0f32, "pm2_5": pm2_5, "pm4_0": null, "pm10": 9.5f32, "humidity": 41.3f32,
               "temperature": 27.1f32, "co2_ppm": co2, "feel": feel})
    }

    /// `air.changed`: the first reading, then only one that moved by a
    /// step, at most every 5 s, the newest held back until then; values the
    /// sensor doesn't know are left out, and `get` `air` says `known`.
    #[test]
    fn the_air_is_told_when_it_moves_at_most_every_five_seconds() {
        let mut watch = Watch::new();
        assert_eq!(watch.state(StateKind::Air), json!({"known": false}));
        let first = watch.take("air/state", &air(12.3, 640, "fresh"), 0);
        assert_eq!(
            first,
            [Out::Event(
                EventKind::AirChanged,
                json!({"pm1_0": 3.0, "pm2_5": 12.3, "pm10": 9.5, "humidity": 41.3,
                       "temperature": 27.1, "co2_ppm": 640, "feel": "fresh"})
            )]
        );
        // Under every step: nothing, however late.
        assert!(
            watch
                .take("air/state", &air(14.0, 680, "fresh"), 6_000_000)
                .is_empty()
        );
        // Past a step but within 5 s of the last told: held back, then told
        // by the check, with the newest reading.
        let mut watch = Watch::new();
        watch.take("air/state", &air(12.3, 640, "fresh"), 0);
        assert!(
            watch
                .take("air/state", &air(12.3, 900, "fine"), 2_000_000)
                .is_empty()
        );
        assert!(watch.lapse(4_000_000).is_empty());
        let later = watch.lapse(5_000_000);
        assert_eq!(kinds(&later), [EventKind::AirChanged]);
        let Out::Event(_, told) = &later[0] else {
            unreachable!()
        };
        assert_eq!(
            (told["co2_ppm"].clone(), told["feel"].clone()),
            (json!(900), json!("fine"))
        );
        assert_eq!(watch.state(StateKind::Air)["known"], json!(true));
        assert_eq!(watch.state(StateKind::Air)["co2_ppm"], json!(900));
        // Not heard for 15 s: no longer known.
        watch.lapse(16_999_999);
        assert_eq!(watch.state(StateKind::Air)["known"], json!(true));
        watch.lapse(17_000_000);
        assert_eq!(watch.state(StateKind::Air), json!({"known": false}));
    }

    /// A value that comes or goes is news, and so is `feel`.
    #[test]
    fn an_air_value_that_comes_or_goes_or_a_new_feel_is_news() {
        let mut watch = Watch::new();
        watch.take(
            "air/state",
            &json!({"pm2_5": null, "co2_ppm": null, "feel": "unknown"}),
            0,
        );
        assert_eq!(
            kinds(&watch.take(
                "air/state",
                &json!({"pm2_5": 4.0f32, "co2_ppm": null, "feel": "unknown"}),
                5_000_000
            )),
            [EventKind::AirChanged]
        );
        assert_eq!(
            kinds(&watch.take(
                "air/state",
                &json!({"pm2_5": 4.0f32, "co2_ppm": 790, "feel": "fresh"}),
                10_000_000
            )),
            [EventKind::AirChanged]
        );
    }

    /// `get` `device`: `board/state`'s numbers with `known`, never told as
    /// an event; `known: false` once not heard for 15 s.
    #[test]
    fn the_device_is_the_boards_newest_numbers() {
        let mut watch = Watch::new();
        assert_eq!(watch.state(StateKind::Device), json!({"known": false}));
        let board = json!({"temperature_c": {"cpu_thermal": 61.5}, "cpu_load": 0.42, "memory_used": 0.55,
                           "disk_free": 0.71, "uptime_s": 3600, "wifi_dbm": -52, "clock_synced": true});
        assert!(watch.take("board/state", &board, 1_000_000).is_empty());
        assert_eq!(
            watch.state(StateKind::Device),
            json!({"temperature_c": {"cpu_thermal": 61.5}, "cpu_load": 0.42, "memory_used": 0.55,
                   "disk_free": 0.71, "uptime_s": 3600, "known": true})
        );
        watch.lapse(16_000_000);
        assert_eq!(watch.state(StateKind::Device), json!({"known": false}));
    }

    #[test]
    fn a_volume_change_from_anyone_is_told_once_and_the_first_heard_is_state() {
        let mut watch = Watch::new();
        let volume = |level: u64, muted: bool| json!({"volume": level, "muted": muted});
        // The one kept on the bus, heard at the start: state, not news.
        assert!(
            watch
                .take("speaker/volume", &volume(40, false), S)
                .is_empty()
        );
        assert_eq!(
            watch.state(crate::protocol::StateKind::Volume),
            json!({"level": 40, "muted": false})
        );
        assert!(
            watch
                .take("speaker/volume", &volume(40, false), 2 * S)
                .is_empty()
        );
        // The person says "louder", the app mutes it: each is told.
        assert_eq!(
            watch.take("speaker/volume", &volume(60, false), 3 * S),
            [Out::Event(
                EventKind::VolumeChanged,
                json!({"level": 60, "muted": false})
            )]
        );
        assert_eq!(
            watch.take("speaker/volume", &volume(0, true), 4 * S),
            [Out::Event(
                EventKind::VolumeChanged,
                json!({"level": 0, "muted": true})
            )]
        );
        // Not a volume: nothing, and the state stands.
        assert!(
            watch
                .take(
                    "speaker/volume",
                    &json!({"volume": 101, "muted": false}),
                    5 * S
                )
                .is_empty()
        );
        assert!(watch.take("speaker/volume", &json!({}), 5 * S).is_empty());
        assert_eq!(
            watch.state(crate::protocol::StateKind::Volume),
            json!({"level": 0, "muted": true})
        );
        assert_eq!(
            Watch::new().state(crate::protocol::StateKind::Volume),
            Value::Null
        );
    }

    /// `get` `body` once lamp-body stops or restarts: limits not said for
    /// 15 s are not known, and a position not heard for 15 s is none, never
    /// the old one (the first builder's P2 on #4580).
    #[test]
    fn the_body_not_heard_for_15_s_is_not_known() {
        let mut watch = Watch::new();
        let limits = json!({"joints": [{"name": "base_yaw", "min": 1975, "max": 2055, "home": 2015}], "speed": 50.0});
        let joints = json!({"name": ["base_yaw"], "position": [0.0]});
        watch.take("body/limits", &limits, S);
        watch.take("body/joint_states", &joints, S);
        assert_eq!(
            watch.state(StateKind::Body)["joints"]["yaw"]["counts"],
            2_048.0
        );
        // Limits said again every 10 s; the positions stop.
        watch.take("body/limits", &limits, 10 * S);
        watch.lapse(S + READING_STALE_US - 1);
        assert_eq!(
            watch.state(StateKind::Body)["joints"]["yaw"]["counts"],
            2_048.0
        );
        watch.lapse(S + READING_STALE_US);
        let body = watch.state(StateKind::Body);
        assert_eq!(body["known"], true, "the limits are still fresh");
        assert!(body["joints"]["yaw"]["counts"].is_null(), "{body}");
        assert!(body["joints"]["yaw"]["deg"].is_null(), "{body}");
        // And the limits stop too.
        watch.lapse(10 * S + READING_STALE_US);
        assert_eq!(watch.state(StateKind::Body)["known"], false);
    }

    /// `name.heard` carries the listener's score, to a hundredth, so a
    /// program can set its own bar (the 75c9 false wake scored 0.941, real
    /// calls 0.90 to 1.00: Queue #5385, 9 Nov).
    #[test]
    fn the_names_score_goes_with_name_heard() {
        let mut watch = Watch::new();
        let heard = |watch: &mut Watch, payload: Value| match watch
            .take("name/heard", &payload, S)
            .as_slice()
        {
            [Out::Event(EventKind::NameHeard, fields)] => fields.clone(),
            other => panic!("{other:?}"),
        };
        assert_eq!(
            heard(&mut watch, json!({"score": 0.9413, "ago_ms": 75})),
            json!({"score": 0.94})
        );
        assert_eq!(
            heard(&mut watch, json!({"score": 1.0, "ago_ms": 360})),
            json!({"score": 1.0})
        );
        // Never past 0 to 1, and none when the bus gives none.
        assert_eq!(
            heard(&mut watch, json!({"score": 1.7})),
            json!({"score": 1.0})
        );
        assert_eq!(heard(&mut watch, json!({"ago_ms": 75})), json!({}));
    }
}
