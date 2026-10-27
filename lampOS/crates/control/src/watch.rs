//! What the lamp's bus says, as the door's events, lease signals and state
//! (docs/control.md, "Events"). Pure: each bus message goes in
//! ([`Watch::take`]) and what it means comes out; the server subscribes and
//! hands them on.
//!
//! - **Events,** by edge, never by every message: someone arrived (a face
//!   after none for [`ABSENT_FOR_US`]) or left (none for [`LEFT_AFTER_US`]),
//!   turned to or from the lamp, stood up; the lamp's name heard; a sound
//!   class rising past [`SOUND_BAR`]; a touch or the button; the lamp
//!   talking, paused, private or asleep.
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
/// The sound classes told, never `speech`.
pub const SOUND_CLASSES: [&str; 4] = ["music", "laughter", "door", "footsteps"];

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
    asleep: bool,
    light: Value,
    mood: Value,
    sounds: [bool; 4],
}

impl Watch {
    pub fn new() -> Self {
        Self::default()
    }

    /// One message on `topic`, its JSON payload, at `now_us`.
    pub fn take(&mut self, topic: &str, payload: &Value, now_us: u64) -> Vec<Out> {
        match topic {
            "humans/faces/tracked" => self.faces(payload, now_us),
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
                let talking = payload["state"]
                    .as_str()
                    .is_some_and(|state| state != "resting");
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
                payload["paused"] == true,
                |watch| &mut watch.paused,
                LeaseWhy::Paused,
            ),
            "voice/mute" => {
                let asleep = payload["muted"] == true && payload["reason"] == "sleep";
                self.flag(asleep, |watch| &mut watch.asleep, LeaseWhy::Asleep)
            }
            "voice/privacy" => {
                let private = payload["permitted"] == false || payload["physical_muted"] == true;
                self.flag(private, |watch| &mut watch.private, LeaseWhy::Private)
            }
            "light/state" => {
                self.light = json!({"on": payload["on"], "colour": payload["colour"], "level": payload["level"]});
                Vec::new()
            }
            "mood/state" => {
                self.mood = json!({"energy": payload["energy"], "warmth": payload["warmth"], "awake": payload["awake"], "asleep": payload["asleep"]});
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// The state `get` answers for `kind`.
    pub fn state(&self, kind: crate::protocol::StateKind) -> Value {
        use crate::protocol::StateKind;
        match kind {
            StateKind::Presence => {
                json!({"count": self.count, "near": self.near, "facing": self.facing})
            }
            StateKind::Light => self.light.clone(),
            StateKind::Mood => self.mood.clone(),
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
}
