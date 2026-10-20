//! What the page keeps of the bus's events: a count and the last time for
//! each topic, a short tail of what came, and the eyes' last word on the
//! camera. Of each message it keeps only what [`what`] says of it: never
//! the words of `voice/heard` or `voice/said`, never a diagnostic's message.
use lamp_bus::{Bus, Message};
use lamp_msgs::{Diagnostic, Speech, VoiceTurn};
use std::{
    collections::{BTreeMap, VecDeque},
    io,
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

/// The event topics the page listens to. The rest it reads as their kept
/// values, which needs no subscriber ([`crate::view::LATEST`]).
pub const EVENTS: [&str; 4] = [
    Speech::TOPICS[0],
    Speech::TOPICS[1],
    VoiceTurn::TOPIC,
    Diagnostic::TOPIC,
];
/// The name its subscribers have on the bus.
pub const SUBSCRIBER: &str = "peek";
/// The most lines the tail keeps.
pub const TAIL: usize = 60;
/// How long the watcher rests when nothing came.
const REST: Duration = Duration::from_millis(20);
/// The part whose diagnostics say how the camera is, and their key.
const EYES: &str = "eyes";
const CAMERA: &str = "camera";
/// The camera's states as the eyes say them (`crates/eyes/src/bus_out.rs`,
/// `health`). Any other value is not taken.
const CAMERA_STATES: [&str; 4] = ["open", "closed", "not opening", "failed"];

/// One message of the tail, as the page shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub at_unix_ms: u64,
    pub topic: String,
    pub source: String,
    pub what: String,
}

/// How many messages came on a topic, and when the last did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Count {
    pub messages: u64,
    pub last_unix_ms: u64,
}

#[derive(Debug, Default)]
pub struct Seen {
    pub counts: BTreeMap<String, Count>,
    /// Newest last.
    pub tail: VecDeque<Line>,
    /// The eyes' last word on the camera, and when it came.
    pub camera: Option<(String, Instant)>,
}

impl Seen {
    /// Count `message` and keep what it says.
    pub fn take(&mut self, message: &Message, now: Instant) {
        let header = &message.header;
        let count = self.counts.entry(header.topic.clone()).or_default();
        count.messages += 1;
        count.last_unix_ms = header.at_unix_ms;
        if header.topic == Diagnostic::TOPIC
            && let Some(state) = camera_of(message)
        {
            self.camera = Some((state, now));
        }
        self.tail.push_back(Line {
            at_unix_ms: header.at_unix_ms,
            topic: header.topic.clone(),
            source: plain(&header.source),
            what: what(message),
        });
        while self.tail.len() > TAIL {
            self.tail.pop_front();
        }
    }
}

/// `text` when it is a plain name (a part, a locale), else `?`: nothing a
/// message carries reaches the page unless it is one.
pub fn plain(text: &str) -> String {
    let plain = !text.is_empty()
        && text.len() <= 40
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_./".contains(&byte));
    if plain { text.to_owned() } else { "?".into() }
}

/// What the page says of a message: its kind of news, never its words.
pub fn what(message: &Message) -> String {
    let topic = message.header.topic.as_str();
    if Speech::TOPICS.contains(&topic) {
        let Ok(speech) = message.parse::<Speech>() else {
            return "unreadable".into();
        };
        let mut what = if speech.finished {
            "finished"
        } else {
            "a piece"
        }
        .to_owned();
        if let Some(request) = speech.request {
            what += &format!(", request {request}");
        }
        if let Some(locale) = &speech.locale {
            what += &format!(", {}", plain(locale));
        }
        return what;
    }
    match topic {
        VoiceTurn::TOPIC => match message.parse::<VoiceTurn>() {
            Ok(turn) => format!("request {}, {}", turn.request, name_of(&turn.verdict)),
            Err(_) => "unreadable".into(),
        },
        Diagnostic::TOPIC => match message.parse::<Diagnostic>() {
            Ok(diagnostic) => {
                let mut what =
                    format!("{} {}", plain(&diagnostic.name), name_of(&diagnostic.level));
                if let Some(state) = camera_of(message) {
                    what += &format!(", camera {state}");
                }
                what
            }
            Err(_) => "unreadable".into(),
        },
        _ => format!("{} bytes", message.payload.len()),
    }
}

/// A value's name as the bus carries it (`held_back`, `warn`).
fn name_of(value: &impl serde::Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(plain))
        .unwrap_or_else(|| "?".into())
}

/// The camera's state, when `message` is the eyes' diagnostic saying it.
pub fn camera_of(message: &Message) -> Option<String> {
    let diagnostic = message.parse::<Diagnostic>().ok()?;
    if diagnostic.name != EYES {
        return None;
    }
    let value = diagnostic.values.iter().find(|value| value.key == CAMERA)?;
    CAMERA_STATES
        .contains(&value.value.as_str())
        .then(|| value.value.clone())
}

/// Listen to [`EVENTS`] on `bus` until `stop` is set, keeping what came in
/// `seen`. Ends with the first error the bus gives.
pub fn watch(bus: &Bus, seen: &Mutex<Seen>, stop: &AtomicBool) -> io::Result<()> {
    let mut subscribers = EVENTS
        .iter()
        .map(|topic| bus.subscribe(topic, SUBSCRIBER))
        .collect::<io::Result<Vec<_>>>()?;
    while !stop.load(Ordering::Relaxed) {
        let mut came = false;
        for subscriber in &mut subscribers {
            while let Some(message) = subscriber.recv(Duration::ZERO)? {
                seen.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .take(&message, Instant::now());
                came = true;
            }
        }
        if !came {
            thread::sleep(REST);
        }
    }
    Ok(())
}
