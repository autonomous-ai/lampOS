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

use crate::protocol::{EventKind, LeaseWhy, Part};

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
    /// Every lease (on a part, or all) suspended while someone talks.
    Suspend(Option<Part>, LeaseWhy),
    /// Talking ended: suspended leases held again.
    Resume,
    /// Every lease (on a part, or all) lost.
    Lose(Option<Part>, LeaseWhy),
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
            "name/heard" => vec![Out::Event(EventKind::NameHeard, json!({}))],
            "voice/heard" => match (payload["text"].as_str(), payload["finished"].as_bool()) {
                (Some(text), Some(true)) if !text.trim().is_empty() => {
                    vec![Out::Heard(text.to_owned())]
                }
                _ => Vec::new(),
            },
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
    let words = crate::protocol::phrase_words;
    let heard = words(heard);
    phrases
        .iter()
        .filter(|phrase| {
            let wanted = words(phrase);
            !wanted.is_empty()
                && heard
                    .windows(wanted.len())
                    .any(|window| window == wanted.as_slice())
        })
        .map(String::as_str)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::StateKind;

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
}
