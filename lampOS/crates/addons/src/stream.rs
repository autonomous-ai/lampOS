//! The stream to the person's own computer (devin-0-P4, the owner, 26 Oct):
//! what the lamp says on its bus, through Harness's `addon.publish`, which
//! keeps only the newest value of a topic. So each change on the bus becomes
//! a numbered line, and the lines are packed into a few rotating slots,
//! `lamp/log/s0` to `lamp/log/s7`, each a value Harness takes (at most
//! 4,096 bytes). A computer that reads the slots at least every few seconds
//! puts the lines back in order by their numbers and misses none; one that
//! reads less often sees the gap, never a line out of place.
//!
//! This module holds the rules alone (which topics, what counts as a change,
//! how lines are packed), with no bus and no clock, so each is tested as a
//! function. `crate::machine` reads the bus and publishes.
use serde_json::{Map, Value, json};
use std::collections::{HashMap, VecDeque};

/// How many slots rotate: about eight seconds of a busy lamp in flight.
pub const SLOTS: usize = 8;
/// A slot's topic is this and its number.
pub const SLOT_TOPIC: &str = "lamp/log/s";
/// A value Harness takes.
const SLOT_BYTES: usize = 4_096;
/// One line's value at most; a larger one is said by its size alone.
const LINE_VALUE_BYTES: usize = 1_024;
/// Lines waiting to be sent at most; older ones are dropped and counted.
const BACKLOG: usize = 4_096;

/// Never followed: raw audio and joint angles, many a second and no use
/// as lines, the privacy lease said every 10 ms, and the slots' own topic.
const RAW: [&str; 3] = ["body/joint_states", "voice/privacy", "lamp/log"];
/// Continuous state, said many times a second: followed only when the
/// person asks for everything.
const BUSY: [&str; 15] = [
    "controls/state",
    "controls/touch_state",
    "expression/body",
    "expression/state",
    "ears/level",
    "voice/activity",
    "voice/progress",
    "voice/timing",
    "voice/sound",
    "voice/playout",
    "light/drawn",
    "humans/faces/tracked",
    "attention/concentration",
    "lamp/health",
    "body/hand",
];
/// What a person said or typed, or what is kept about a person: followed
/// only with the yes for words.
const WORDS: [&str; 7] = [
    "voice/heard",
    "voice/said",
    "chat/",
    "memory/",
    "humans/",
    "skills/ask",
    "messages/",
];

/// What the person's yes allows to be followed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Allowed {
    /// Also what was said or typed, and what is kept about a person.
    pub words: bool,
    /// Also the continuous state streams.
    pub busy: bool,
}

/// Whether `topic` is followed.
pub fn followed(topic: &str, allowed: Allowed) -> bool {
    let under = |prefix: &str| {
        topic == prefix
            || topic.starts_with(prefix)
                && (prefix.ends_with('/') || topic[prefix.len()..].starts_with('/'))
    };
    if topic.ends_with("/audio") || RAW.iter().any(|raw| under(raw)) {
        return false;
    }
    if !allowed.busy && BUSY.contains(&topic) {
        return false;
    }
    allowed.words || !WORDS.iter().any(|words| under(words))
}

/// A value's meaning: the value without the times, counters, leases and
/// stamps that change in every message. Two messages of the same meaning
/// are one line. A part's diagnostics count only by their level and message.
pub fn meaning(topic: &str, value: &Value) -> (String, String) {
    if topic == "diagnostics" {
        let name = value.get("name").and_then(Value::as_str).unwrap_or("");
        let said = json!([value.get("level"), value.get("message")]);
        return (format!("{topic}#{name}"), said.to_string());
    }
    (topic.to_owned(), steady(value).to_string())
}

fn steady(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .filter(|(key, _)| !volatile(key))
                .map(|(key, value)| (key.clone(), steady(value)))
                .collect::<Map<_, _>>(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(steady).collect()),
        other => other.clone(),
    }
}

fn volatile(key: &str) -> bool {
    matches!(
        key,
        "stamp" | "lease" | "seq" | "sequence" | "serial" | "generation"
    ) || ["_us", "_ms", "_at"].iter().any(|end| key.ends_with(end))
        || key.starts_with("issued")
        || key.starts_with("expires")
}

/// One line: its number, when the lamp said it (Unix ms), the topic, the value.
#[derive(Clone, Debug, PartialEq)]
struct Line {
    seq: u64,
    at: u64,
    topic: String,
    value: Value,
}

impl Line {
    fn json(&self) -> Value {
        json!([self.seq, self.at, self.topic, self.value])
    }
}

/// The lines not yet sent, and which slot comes next.
#[derive(Debug, Default)]
pub struct Stream {
    next: u64,
    lines: VecDeque<Line>,
    last: HashMap<String, String>,
    slot: usize,
    dropped: u64,
}

/// Slots made for one publish: sent with [`Stream::sent`] once Harness took them.
#[derive(Clone, Debug, PartialEq)]
pub struct Packed {
    pub values: Vec<(String, Value)>,
    through: u64,
}

impl Stream {
    /// One message from the bus: a line, unless it means what the last one
    /// of its topic meant. Says whether it became one.
    pub fn take(&mut self, topic: &str, value: Value, at_unix_ms: u64) -> bool {
        let (key, meaning) = meaning(topic, &value);
        if self.last.get(&key) == Some(&meaning) {
            return false;
        }
        self.last.insert(key, meaning);
        let size = value.to_string().len();
        let value = if size > LINE_VALUE_BYTES {
            json!({ "too_big": size })
        } else {
            value
        };
        self.next += 1;
        self.lines.push_back(Line {
            seq: self.next,
            at: at_unix_ms,
            topic: topic.to_owned(),
            value,
        });
        while self.lines.len() > BACKLOG {
            self.lines.pop_front();
            self.dropped += 1;
        }
        true
    }

    /// What a topic last meant is forgotten, so its next message is a line:
    /// after the yes changed, or the computer's copy was cleared.
    pub fn forget(&mut self) {
        self.last.clear();
    }

    /// The waiting lines in at most [`SLOTS`] slots, each a value Harness
    /// takes. Nothing is removed until [`Stream::sent`].
    pub fn pack(&self) -> Option<Packed> {
        if self.lines.is_empty() && self.dropped == 0 {
            return None;
        }
        let mut values = Vec::new();
        let mut lines = self.lines.iter().peekable();
        let mut through = 0;
        let mut dropped = self.dropped;
        while values.len() < SLOTS && (lines.peek().is_some() || dropped > 0) {
            let mut slot = Vec::new();
            // The envelope's own size, with room for its numbers.
            let mut size = 64;
            while let Some(line) = lines.peek() {
                let text = line.json().to_string().len() + 1;
                if size + text > SLOT_BYTES && !slot.is_empty() {
                    break;
                }
                size += text;
                through = line.seq;
                slot.push(line.json());
                lines.next();
            }
            let from = slot
                .first()
                .and_then(|line| line.get(0))
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let topic = format!("{SLOT_TOPIC}{}", (self.slot + values.len()) % SLOTS);
            values.push((
                topic,
                json!({ "from": from, "to": through, "dropped": dropped, "lines": slot }),
            ));
            dropped = 0;
        }
        Some(Packed { values, through })
    }

    /// Harness took `packed`: its lines are gone, and the next slot follows.
    pub fn sent(&mut self, packed: &Packed) {
        while self
            .lines
            .front()
            .is_some_and(|line| line.seq <= packed.through)
        {
            self.lines.pop_front();
        }
        self.slot = (self.slot + packed.values.len()) % SLOTS;
        self.dropped = 0;
    }

    /// Lines made so far.
    pub fn made(&self) -> u64 {
        self.next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: Allowed = Allowed {
        words: true,
        busy: true,
    };

    #[test]
    fn raw_streams_never_and_words_and_busy_state_only_when_allowed() {
        let some = Allowed::default();
        assert!(followed("voice/turn", some));
        assert!(followed("addon/decision", some));
        assert!(followed("diagnostics", some));
        assert!(!followed("mic2/audio", ALL));
        assert!(!followed("body/joint_states", ALL));
        assert!(!followed("voice/privacy", ALL));
        assert!(!followed("lamp/log/s3", ALL));
        assert!(!followed("voice/heard", some));
        assert!(!followed("chat/in", some));
        assert!(!followed("humans/faces/tracked", some));
        assert!(followed("voice/heard", ALL));
        assert!(!followed("expression/body", some));
        assert!(followed("expression/body", ALL));
        // A prefix names a whole segment, never part of one.
        assert!(followed("voice/heard_count", some));
        assert!(followed("lamp/logs", some));
    }

    #[test]
    fn a_change_is_a_change_of_meaning() {
        let mut stream = Stream::default();
        assert!(stream.take("voice/state", json!({"state": "resting"}), 1));
        assert!(!stream.take("voice/state", json!({"state": "resting"}), 2));
        assert!(stream.take("voice/state", json!({"state": "listening"}), 3));
        let at = |us: u64, n: u64| json!({"on": true, "stamp": {"at_us": us}, "sequence": n, "issued_us": us});
        assert!(stream.take("light/state", at(1, 1), 4));
        assert!(!stream.take("light/state", at(2, 2), 5));
        let diagnostic = |name: &str, level: &str, count: u64| json!({"name": name, "level": level, "message": "", "values": [{"key": "n", "value": count}]});
        assert!(stream.take("diagnostics", diagnostic("body", "ok", 1), 6));
        assert!(stream.take("diagnostics", diagnostic("eyes", "ok", 1), 7));
        assert!(!stream.take("diagnostics", diagnostic("body", "ok", 2), 8));
        assert!(stream.take("diagnostics", diagnostic("body", "warn", 2), 9));
        assert_eq!(stream.made(), 6);
        stream.forget();
        assert!(stream.take("voice/state", json!({"state": "listening"}), 10));
    }

    #[test]
    fn lines_are_packed_in_order_into_slots_harness_takes_and_go_once_sent() {
        let mut stream = Stream::default();
        for n in 0..200 {
            stream.take(
                "voice/turn",
                json!({"request": n, "verdict": "answered"}),
                1_000 + n,
            );
        }
        let packed = stream.pack().unwrap();
        assert!(packed.values.len() > 1 && packed.values.len() <= SLOTS);
        let mut seen = 0;
        for (index, (topic, value)) in packed.values.iter().enumerate() {
            assert_eq!(topic, &format!("lamp/log/s{index}"));
            assert!(value.to_string().len() <= SLOT_BYTES);
            for line in value["lines"].as_array().unwrap() {
                seen += 1;
                assert_eq!(line[0], seen);
            }
        }
        assert_eq!(seen, 200);
        assert_eq!(
            stream.pack(),
            Some(packed.clone()),
            "nothing goes until sent"
        );
        stream.sent(&packed);
        assert_eq!(stream.pack(), None);
        stream.take("voice/state", json!({"state": "thinking"}), 2_000);
        let next = stream.pack().unwrap();
        assert_eq!(
            next.values[0].0,
            format!("lamp/log/s{}", packed.values.len() % SLOTS)
        );
        assert_eq!(next.values[0].1["from"], 201);
    }

    #[test]
    fn a_large_value_is_said_by_its_size_and_a_flood_is_counted() {
        let mut stream = Stream::default();
        let large = json!({"text": "x".repeat(5_000)});
        let size = large.to_string().len();
        stream.take("memory/read", large, 1);
        let packed = stream.pack().unwrap();
        assert_eq!(packed.values[0].1["lines"][0][3], json!({"too_big": size}));
        let mut flood = Stream::default();
        for n in 0..(BACKLOG as u64 + 10) {
            flood.take("voice/turn", json!({"request": n}), n);
        }
        let packed = flood.pack().unwrap();
        assert_eq!(packed.values[0].1["dropped"], 10);
        assert_eq!(packed.values[0].1["from"], 11);
        assert_eq!(
            packed.values.len(),
            SLOTS,
            "a flood fills every slot, the rest waits"
        );
        flood.sent(&packed);
        assert_eq!(flood.pack().unwrap().values[0].1["dropped"], 0);
    }
}
