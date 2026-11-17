//! The owner's desk and Hydrate bottles, through the cloud (#7470): the
//! lamp half of `devices.*`. With `LAMP_DEVICES=on` ([`ENVIRONMENT`]) and
//! `LAMP_CLOUD`, `lamp-cloud` takes each [`DevicesAsk`] the voice puts on
//! `devices/ask`, sends it up on the lamp's own channel (`devices.list.get`,
//! `devices.state.get`, `devices.command`, each with its `req_id`), and puts
//! the cloud's reply (`devices.list`, `devices.state`,
//! `devices.command.result`) on `devices/answer` under the same `req_id`.
//! The cloud's news (`devices.event`) goes on `devices/event`. Off, nothing
//! is taken and nothing is sent.
//!
//! Only the voice's asks are taken ([`VOICE`], its publisher name). The lamp
//! asks about and commands only a device the cloud's own list gave it
//! (the cloud checks the owner, and says `refused` for any other); a desk
//! command goes to a desk only; an ask about a device not listed is answered
//! `unlisted` here and never sent.
//!
//! Bounded: at most [`MOST_PENDING`] asks wait for an answer and at most
//! [`PER_MINUTE`] are taken a minute, the rest answered `busy`; an ask the
//! cloud does not answer in [`ANSWER_WITHIN_MS`] is answered `timeout`, and a
//! late reply is dropped. A command is sent at most once, never again after a
//! reconnect: with the broker away it is answered `away` at once, not kept.
//!
//! `stop` goes first: it skips the caps, waits behind nothing, and cancels
//! every command still waiting (never sent), so no move follows it.
use lamp_msgs::{
    DeviceAnswer, DeviceAsk, DeviceCommand, DeviceRow, DeviceState, DeviceStatus, DeviceType,
    DevicesAnswer, DevicesAsk, DevicesEvent, NoAnswer,
};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet, VecDeque};

/// Where `lamp-cloud` reads whether the devices go through: `on` only.
pub const ENVIRONMENT: &str = "LAMP_DEVICES";
/// The voice's publisher name: an ask from any other is not taken.
pub const VOICE: &str = "voice";

/// What the lamp sends up.
pub const LIST_GET: &str = "devices.list.get";
pub const STATE_GET: &str = "devices.state.get";
pub const COMMAND: &str = "devices.command";
/// What the cloud sends down.
pub const LIST: &str = "devices.list";
pub const STATE: &str = "devices.state";
pub const COMMAND_RESULT: &str = "devices.command.result";
pub const EVENT: &str = "devices.event";

/// Whether the environment turns the devices on.
pub fn on_from_environment() -> bool {
    on_from(std::env::var(ENVIRONMENT).ok().as_deref())
}

/// Exactly `on`, nothing else.
pub fn on_from(value: Option<&str>) -> bool {
    value == Some("on")
}

/// Whether `kind` is one the cloud sends down for the devices.
pub fn is_down(kind: &str) -> bool {
    matches!(kind, LIST | STATE | COMMAND_RESULT | EVENT)
}

/// Asks waiting for an answer at most.
pub const MOST_PENDING: usize = 8;
/// Asks taken in a minute at most (a stop is not counted).
pub const PER_MINUTE: usize = 20;
/// Stops in a minute at most: a flood of stops is still a flood.
pub const STOPS_PER_MINUTE: usize = 60;
/// The most events passed down in a minute.
pub const EVENTS_PER_MINUTE: usize = 30;
/// An ask not answered by then is answered `timeout`, in ms.
pub const ANSWER_WITHIN_MS: u64 = 8_000;
/// The most other publishers the journal names, once each.
const REFUSED_NAMED: usize = 8;
const MINUTE_MS: u64 = 60_000;
/// The most publisher boots remembered for `seq`.
const MOST_BOOTS: usize = 256;

/// What became of an ask heard on `devices/ask`.
#[derive(Debug, PartialEq, Eq)]
pub enum Taken {
    /// Waiting to be sent, or answered at once (an `Unlisted`, `Busy`).
    Kept,
    /// Not the voice's: the publisher's name the first time, for the
    /// journal, and `None` after.
    NotVoice(Option<String>),
    /// Not a valid ask.
    Invalid,
    /// Heard before (the same publisher and boot, a `seq` not past its last).
    Again,
    /// Its `req_id` is already waiting or out.
    Duplicate,
}

#[derive(Debug)]
struct Out {
    ask: DevicesAsk,
    sent_ms: u64,
}

/// One thing to put on the lamp's channel: the `kind` and its `data`.
#[derive(Debug, PartialEq)]
pub struct Send {
    pub kind: &'static str,
    pub data: Value,
}

/// The asks waiting and out, and what bounds them. No I/O: [`Link`] does it.
#[derive(Debug, Default)]
pub struct Bridge {
    /// Not yet sent, in order, a stop at the front.
    waiting: VecDeque<DevicesAsk>,
    /// Sent, by `req_id`.
    out: HashMap<String, Out>,
    /// The devices the cloud's last list gave, and what each is.
    known: HashMap<String, DeviceType>,
    /// When each ask taken in the last minute was, in ms.
    taken_ms: VecDeque<u64>,
    stops_ms: VecDeque<u64>,
    events_ms: VecDeque<u64>,
    last: HashMap<(String, String), u64>,
    refused: HashSet<String>,
    /// Answers to put on `devices/answer`, oldest first.
    answers: VecDeque<DevicesAnswer>,
    /// News to put on `devices/event`.
    events: VecDeque<DevicesEvent>,
    /// The journal's lines, never an id's owner's name or a reading.
    lines: Vec<String>,
    /// A reply that matched nothing was journalled since the last drain.
    stray_said: bool,
}

impl Bridge {
    /// Takes `ask`, from `source`'s `boot` as `seq`, at `now_ms`.
    pub fn take(
        &mut self,
        (source, boot, seq): (&str, &str, u64),
        ask: DevicesAsk,
        now_ms: u64,
    ) -> Taken {
        if source != VOICE {
            let first =
                self.refused.len() < REFUSED_NAMED && self.refused.insert(source.to_owned());
            return Taken::NotVoice(first.then(|| name_of(source)));
        }
        if ask.check().is_err() {
            return Taken::Invalid;
        }
        let key = (source.to_owned(), boot.to_owned());
        if self.last.get(&key).is_some_and(|last| seq <= *last) {
            return Taken::Again;
        }
        // A voice call makes a publisher of its own, so a boot a call: the
        // memory of them is bounded.
        if self.last.len() >= MOST_BOOTS {
            self.last.clear();
        }
        self.last.insert(key, seq);
        if self.out.contains_key(&ask.req_id) || self.waiting.iter().any(|w| w.req_id == ask.req_id)
        {
            return Taken::Duplicate;
        }
        let stop = matches!(
            ask.ask,
            DeviceAsk::Command {
                command: DeviceCommand::Stop,
                ..
            }
        );
        window(&mut self.taken_ms, now_ms);
        window(&mut self.stops_ms, now_ms);
        if stop {
            if self.stops_ms.len() >= STOPS_PER_MINUTE {
                self.answer(
                    &ask.req_id,
                    DeviceAnswer::None {
                        why: NoAnswer::Busy,
                    },
                );
                return Taken::Kept;
            }
            self.stops_ms.push_back(now_ms);
            // Nothing waiting may move the desk after this stop.
            let mut kept = VecDeque::new();
            for waiting in std::mem::take(&mut self.waiting) {
                if matches!(waiting.ask, DeviceAsk::Command { .. }) {
                    self.answer(
                        &waiting.req_id,
                        DeviceAnswer::None {
                            why: NoAnswer::Cancelled,
                        },
                    );
                } else {
                    kept.push_back(waiting);
                }
            }
            self.waiting = kept;
            self.waiting.push_front(ask);
            return Taken::Kept;
        }
        if let Some(why) = self.refuse(&ask) {
            self.answer(&ask.req_id, DeviceAnswer::None { why });
            return Taken::Kept;
        }
        if self.taken_ms.len() >= PER_MINUTE || self.out.len() + self.waiting.len() >= MOST_PENDING
        {
            self.answer(
                &ask.req_id,
                DeviceAnswer::None {
                    why: NoAnswer::Busy,
                },
            );
            return Taken::Kept;
        }
        self.taken_ms.push_back(now_ms);
        self.waiting.push_back(ask);
        Taken::Kept
    }

    /// Why a non-stop ask is not sent: a device the cloud's list did not
    /// give, or a height for anything but a desk.
    fn refuse(&self, ask: &DevicesAsk) -> Option<NoAnswer> {
        match &ask.ask {
            DeviceAsk::List => None,
            DeviceAsk::State { device_id } => {
                (!self.known.contains_key(device_id)).then_some(NoAnswer::Unlisted)
            }
            DeviceAsk::Command { device_id, .. } => {
                (self.known.get(device_id) != Some(&DeviceType::Desk)).then_some(NoAnswer::Unlisted)
            }
        }
    }

    /// What to send now, at `now_ms`, with the broker `connected` or not.
    /// Not connected: nothing is kept for later, every ask waiting is
    /// answered `away`. Also answers the asks out too long `timeout`.
    pub fn due(&mut self, now_ms: u64, connected: bool) -> Vec<Send> {
        let late: Vec<String> = self
            .out
            .iter()
            .filter(|(_, out)| now_ms.saturating_sub(out.sent_ms) >= ANSWER_WITHIN_MS)
            .map(|(id, _)| id.clone())
            .collect();
        for id in late {
            self.out.remove(&id);
            self.lines
                .push("devices: no answer from the cloud in time".into());
            self.answer(
                &id,
                DeviceAnswer::None {
                    why: NoAnswer::Timeout,
                },
            );
        }
        let mut sends = Vec::new();
        for ask in std::mem::take(&mut self.waiting) {
            if !connected {
                self.answer(
                    &ask.req_id,
                    DeviceAnswer::None {
                        why: NoAnswer::Away,
                    },
                );
                continue;
            }
            let send = up(&ask);
            self.lines.push(format!("devices: sent {}", send.kind));
            self.out.insert(
                ask.req_id.clone(),
                Out {
                    ask,
                    sent_ms: now_ms,
                },
            );
            sends.push(send);
        }
        sends
    }

    /// One `devices.*` kind from the cloud with its `data`, at `now_ms`:
    /// an answer to an ask out, or news.
    pub fn down(&mut self, kind: &str, data: &Value, now_ms: u64) {
        if kind == EVENT {
            self.news(data, now_ms);
            return;
        }
        let Some(req_id) = data.get("req_id").and_then(Value::as_str) else {
            return self.stray(kind);
        };
        let Some(out) = self.out.get(req_id) else {
            return self.stray(kind);
        };
        let answer = match (&out.ask.ask, kind) {
            (DeviceAsk::List, LIST) => {
                let rows = rows(data);
                self.known = rows
                    .iter()
                    .map(|row| (row.device_id.clone(), row.device_type))
                    .collect();
                DeviceAnswer::List { devices: rows }
            }
            (DeviceAsk::State { device_id }, STATE) if same(data, device_id) => {
                DeviceAnswer::State {
                    device_id: device_id.clone(),
                    status: status_of(data),
                    state: state_of(data.get("state")),
                }
            }
            (DeviceAsk::Command { device_id, .. }, COMMAND_RESULT) if same(data, device_id) => {
                DeviceAnswer::Command {
                    device_id: device_id.clone(),
                    status: status_of(data),
                    reason: data
                        .get("reason")
                        .and_then(Value::as_str)
                        .map(name_of)
                        .filter(|reason| !reason.is_empty()),
                }
            }
            _ => return self.stray(kind),
        };
        let req_id = req_id.to_owned();
        self.out.remove(&req_id);
        self.lines.push(format!("devices: {kind} answered"));
        self.answer(&req_id, answer);
    }

    fn news(&mut self, data: &Value, now_ms: u64) {
        window(&mut self.events_ms, now_ms);
        if self.events_ms.len() >= EVENTS_PER_MINUTE {
            return;
        }
        let device_type = match data.get("device_type").and_then(Value::as_str) {
            Some("desk") => DeviceType::Desk,
            Some("hydrate") => DeviceType::Hydrate,
            _ => return,
        };
        let text = |key: &str| match data.get(key) {
            Some(Value::String(text)) => text.clone(),
            Some(Value::Number(number)) => number.to_string(),
            _ => String::new(),
        };
        let event = DevicesEvent {
            device_id: text("device_id"),
            device_type,
            event: text("event"),
            at: text("at"),
            data: data.get("data").cloned().unwrap_or(Value::Null),
        };
        if event.check().is_ok() {
            self.events_ms.push_back(now_ms);
            self.events.push_back(event);
        }
    }

    /// A reply that matches nothing out: late, or not ours. Journalled once
    /// until the next drain.
    fn stray(&mut self, kind: &str) {
        if !std::mem::replace(&mut self.stray_said, true) {
            self.lines
                .push(format!("devices: a {kind} matched no ask, dropped"));
        }
    }

    fn answer(&mut self, req_id: &str, answer: DeviceAnswer) {
        self.answers.push_back(DevicesAnswer {
            req_id: req_id.to_owned(),
            answer,
        });
    }

    /// The answers and news to put on the bus, and the journal's lines.
    pub fn drain(&mut self) -> Drained {
        self.stray_said = false;
        Drained {
            answers: self.answers.drain(..).collect(),
            events: self.events.drain(..).collect(),
            lines: std::mem::take(&mut self.lines),
        }
    }

    /// The asks waiting or out.
    pub fn pending(&self) -> usize {
        self.waiting.len() + self.out.len()
    }
}

/// What [`Bridge::drain`] hands back.
#[derive(Debug, Default)]
pub struct Drained {
    pub answers: Vec<DevicesAnswer>,
    pub events: Vec<DevicesEvent>,
    pub lines: Vec<String>,
}

/// The ask as the cloud takes it.
fn up(ask: &DevicesAsk) -> Send {
    match &ask.ask {
        DeviceAsk::List => Send {
            kind: LIST_GET,
            data: json!({"req_id": ask.req_id}),
        },
        DeviceAsk::State { device_id } => Send {
            kind: STATE_GET,
            data: json!({"req_id": ask.req_id, "device_id": device_id}),
        },
        DeviceAsk::Command { device_id, command } => {
            let mut data = json!({
                "req_id": ask.req_id,
                "device_id": device_id,
                "command": command.name(),
            });
            if let DeviceCommand::GotoHeight { cm } = command {
                data["data"] = json!({"value": cm.to_string()});
            }
            Send {
                kind: COMMAND,
                data,
            }
        }
    }
}

fn same(data: &Value, device_id: &str) -> bool {
    data.get("device_id").and_then(Value::as_str) == Some(device_id)
}

fn status_of(data: &Value) -> DeviceStatus {
    match data.get("status").and_then(Value::as_str) {
        Some("ok") => DeviceStatus::Ok,
        Some("refused") => DeviceStatus::Refused,
        _ => DeviceStatus::Error,
    }
}

/// The devices of a `devices.list`: a row of a kind the lamp does not know,
/// or with an id it could not carry, is left out.
fn rows(data: &Value) -> Vec<DeviceRow> {
    let Some(list) = data.get("devices").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|row| {
            let device_type = match row.get("device_type")?.as_str()? {
                "desk" => DeviceType::Desk,
                "hydrate" => DeviceType::Hydrate,
                _ => return None,
            };
            let row = DeviceRow {
                device_id: row.get("device_id")?.as_str()?.to_owned(),
                device_type,
                name: line_of(row.get("name").and_then(Value::as_str).unwrap_or("")),
                commands: row
                    .get("commands")
                    .and_then(Value::as_array)
                    .map(|commands| {
                        commands
                            .iter()
                            .filter_map(Value::as_str)
                            .map(name_of)
                            .take(8)
                            .collect()
                    })
                    .unwrap_or_default(),
            };
            // `check` of the whole answer would refuse the list; one row's
            // odd id is dropped alone.
            DevicesAnswer {
                req_id: "r".into(),
                answer: DeviceAnswer::List {
                    devices: vec![row.clone()],
                },
            }
            .check()
            .ok()
            .map(|()| row)
        })
        .take(lamp_msgs::MOST_DEVICES)
        .collect()
}

fn state_of(state: Option<&Value>) -> DeviceState {
    let number = |key: &str| {
        state
            .and_then(|state| state.get(key))
            .and_then(Value::as_f64)
            .filter(|n| n.is_finite() && *n >= 0.0)
            .map(f64::round)
    };
    DeviceState {
        online: state
            .and_then(|state| state.get("online"))
            .and_then(Value::as_bool),
        height_cm: number("height_cm")
            .filter(|n| *n <= 300.0)
            .map(|n| n as u16),
        today_ml: number("today_ml").filter(|n| *n <= 1e7).map(|n| n as u32),
        sips_today: number("sips_today").filter(|n| *n <= 1e7).map(|n| n as u32),
        goal_ml: number("goal_ml").filter(|n| *n <= 1e7).map(|n| n as u32),
    }
}

/// One line: no control byte, at most 64 characters.
fn line_of(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(64).collect()
}

/// Only a name's letters, digits, `-`, `_` and `.`, at most 48, for a
/// journal or a reason.
fn name_of(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .take(48)
        .collect()
}

fn window(times: &mut VecDeque<u64>, now_ms: u64) {
    while times
        .front()
        .is_some_and(|at| now_ms.saturating_sub(*at) >= MINUTE_MS)
    {
        times.pop_front();
    }
}

/// The journal's line for an ask refused, and why: never its content.
pub fn dropped_line(taken: &Taken) -> Option<String> {
    match taken {
        Taken::NotVoice(Some(source)) => Some(format!(
            "devices: an ask on devices/ask from the publisher {source} wasn't sent: \
             only the voice's are, and later ones from it aren't journalled"
        )),
        Taken::Invalid => Some("devices: an ask that is not valid wasn't sent".into()),
        Taken::Duplicate => Some("devices: an ask with a req_id already in use wasn't sent".into()),
        Taken::Kept | Taken::Again | Taken::NotVoice(None) => None,
    }
}

/// The bus side: the voice's asks listened for, the answers and news put
/// back. Made only with `LAMP_DEVICES=on` (the lamp has none otherwise, and
/// listens for nothing).
#[derive(Default)]
pub struct Link {
    pub bridge: Bridge,
    asks: Option<lamp_bus::Subscriber>,
    answers: Option<lamp_bus::Publisher>,
    events: Option<lamp_bus::Publisher>,
}

/// What [`Link::poll`] hands to the session: what to send up, and the
/// journal's lines.
#[derive(Debug, Default)]
pub struct Polled {
    pub sends: Vec<Send>,
    pub lines: Vec<String>,
}

impl Link {
    /// Takes the asks on the bus, sends what is due up (nothing, answered
    /// `away`, with the broker not `connected`), and puts the answers back.
    pub fn poll(&mut self, bus: &lamp_bus::Bus, now_ms: u64, connected: bool) -> Polled {
        if self.asks.is_none() {
            self.asks = bus
                .subscribe_typed(DevicesAsk::TOPIC, crate::lamp::SOURCE, DevicesAsk::TYPE)
                .ok();
        }
        // The answers' and news' publishers exist before the first ask, so
        // a voice listening for the answer finds them (a late publisher is
        // found only on the subscriber's next rescan).
        if self.answers.is_none() {
            self.answers = bus
                .publisher_declared(
                    DevicesAnswer::TOPIC,
                    &DevicesAnswer::declaration(),
                    crate::lamp::SOURCE,
                )
                .ok();
        }
        if self.events.is_none() {
            self.events = bus
                .publisher_declared(
                    DevicesEvent::TOPIC,
                    &DevicesEvent::declaration(),
                    crate::lamp::SOURCE,
                )
                .ok();
        }
        let mut lines = Vec::new();
        if let Some(asks) = self.asks.as_mut() {
            while let Ok(Some(message)) = asks.recv(std::time::Duration::ZERO) {
                let Ok(ask) = message.parse::<DevicesAsk>() else {
                    continue;
                };
                let header = &message.header;
                let taken =
                    self.bridge
                        .take((&header.source, &header.boot, header.seq), ask, now_ms);
                lines.extend(dropped_line(&taken));
            }
        }
        let sends = self.bridge.due(now_ms, connected);
        lines.extend(self.flush(bus));
        Polled { sends, lines }
    }

    /// One `devices.*` kind from the cloud: its answer or news is on the
    /// bus before this returns.
    pub fn down(&mut self, bus: &lamp_bus::Bus, kind: &str, data: &Value, now_ms: u64) {
        self.bridge.down(kind, data, now_ms);
        // The journal's lines wait for the next poll.
        let lines = self.flush(bus);
        self.bridge.lines.extend(lines);
    }

    fn flush(&mut self, bus: &lamp_bus::Bus) -> Vec<String> {
        let mut drained = self.bridge.drain();
        for answer in drained.answers {
            if answer.check().is_err() {
                continue;
            }
            let out = match &mut self.answers {
                Some(out) => Some(out),
                empty => bus
                    .publisher_declared(
                        DevicesAnswer::TOPIC,
                        &DevicesAnswer::declaration(),
                        crate::lamp::SOURCE,
                    )
                    .ok()
                    .map(|out| empty.insert(out)),
            };
            if out
                .map(|out| out.publish_json(lamp_ipc::monotonic_us(), &answer))
                .is_none_or(|published| published.is_err())
            {
                drained
                    .lines
                    .push("devices: an answer could not be put on the bus".into());
            }
        }
        for event in drained.events {
            let out = match &mut self.events {
                Some(out) => Some(out),
                empty => bus
                    .publisher_declared(
                        DevicesEvent::TOPIC,
                        &DevicesEvent::declaration(),
                        crate::lamp::SOURCE,
                    )
                    .ok()
                    .map(|out| empty.insert(out)),
            };
            if let Some(out) = out {
                let _ = out.publish_json(lamp_ipc::monotonic_us(), &event);
            }
        }
        drained.lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: u64 = 1_000_000;

    /// The cloud, as Backend2 and Backend3 posted it on #7470: a desk and a
    /// Hydrate of the owner's, answered by `req_id`.
    struct FakeCloud {
        /// What `devices.command` answers: `ok`, `refused`, `error`.
        command: &'static str,
        reason: Option<&'static str>,
    }

    impl FakeCloud {
        fn ok() -> Self {
            Self {
                command: "ok",
                reason: None,
            }
        }

        /// The reply to one send.
        fn reply(&self, send: &Send) -> (&'static str, Value) {
            let req_id = send.data["req_id"].clone();
            match send.kind {
                LIST_GET => (
                    LIST,
                    json!({"req_id": req_id, "devices": [
                        {"device_id": "desk-1", "device_type": "desk", "name": "Will Desk Pro AI",
                         "commands": ["goto_height", "memory_position", "stop"]},
                        {"device_id": "hyd-1", "device_type": "hydrate", "name": "Bottle", "commands": []},
                        {"device_id": "toaster", "device_type": "toaster", "name": "?", "commands": []},
                    ]}),
                ),
                STATE_GET => {
                    let state = if send.data["device_id"] == "desk-1" {
                        json!({"height_cm": null, "online": false})
                    } else {
                        json!({"today_ml": 1200, "sips_today": 5, "goal_ml": 2000, "online": true})
                    };
                    (
                        STATE,
                        json!({"req_id": req_id, "device_id": send.data["device_id"],
                               "status": "ok", "state": state}),
                    )
                }
                COMMAND => {
                    let mut data = json!({"req_id": req_id, "device_id": send.data["device_id"],
                                          "status": self.command});
                    if let Some(reason) = self.reason {
                        data["reason"] = reason.into();
                    }
                    (COMMAND_RESULT, data)
                }
                other => panic!("sent {other}"),
            }
        }
    }

    fn ask(req_id: &str, ask: DeviceAsk) -> DevicesAsk {
        DevicesAsk {
            req_id: req_id.into(),
            ask,
        }
    }

    fn goto(req_id: &str, device: &str, cm: u16) -> DevicesAsk {
        ask(
            req_id,
            DeviceAsk::Command {
                device_id: device.into(),
                command: DeviceCommand::GotoHeight { cm },
            },
        )
    }

    fn stop(req_id: &str, device: &str) -> DevicesAsk {
        ask(
            req_id,
            DeviceAsk::Command {
                device_id: device.into(),
                command: DeviceCommand::Stop,
            },
        )
    }

    fn voice(bridge: &mut Bridge, seq: &mut u64, ask: DevicesAsk, now: u64) -> Taken {
        *seq += 1;
        bridge.take((VOICE, "b1", *seq), ask, now)
    }

    /// A bridge that has been given the owner's list.
    fn listed(cloud: &FakeCloud, seq: &mut u64) -> Bridge {
        let mut bridge = Bridge::default();
        voice(&mut bridge, seq, ask("l", DeviceAsk::List), T0);
        for send in bridge.due(T0, true) {
            let (kind, data) = cloud.reply(&send);
            bridge.down(kind, &data, T0);
        }
        let drained = bridge.drain();
        assert!(matches!(
            &drained.answers[0].answer,
            DeviceAnswer::List { devices } if devices.len() == 2
        ));
        bridge
    }

    fn round(bridge: &mut Bridge, cloud: &FakeCloud, now: u64) -> Vec<DevicesAnswer> {
        for send in bridge.due(now, true) {
            let (kind, data) = cloud.reply(&send);
            bridge.down(kind, &data, now);
        }
        bridge.drain().answers
    }

    #[test]
    fn the_list_keeps_the_owners_desk_and_bottle_and_drops_a_kind_it_does_not_know() {
        let mut seq = 0;
        let mut bridge = listed(&FakeCloud::ok(), &mut seq);
        // Known now: a state of the desk is asked, one of a toaster is not.
        voice(
            &mut bridge,
            &mut seq,
            ask(
                "s1",
                DeviceAsk::State {
                    device_id: "desk-1".into(),
                },
            ),
            T0,
        );
        voice(
            &mut bridge,
            &mut seq,
            ask(
                "s2",
                DeviceAsk::State {
                    device_id: "toaster".into(),
                },
            ),
            T0,
        );
        let sent = bridge.due(T0, true);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].kind, STATE_GET);
        assert_eq!(sent[0].data, json!({"req_id": "s1", "device_id": "desk-1"}));
        assert_eq!(
            bridge.drain().answers,
            vec![DevicesAnswer {
                req_id: "s2".into(),
                answer: DeviceAnswer::None {
                    why: NoAnswer::Unlisted
                }
            }]
        );
    }

    #[test]
    fn a_state_answers_what_the_cloud_said_and_a_missing_height_stays_missing() {
        let cloud = FakeCloud::ok();
        let mut seq = 0;
        let mut bridge = listed(&cloud, &mut seq);
        voice(
            &mut bridge,
            &mut seq,
            ask(
                "d",
                DeviceAsk::State {
                    device_id: "desk-1".into(),
                },
            ),
            T0,
        );
        voice(
            &mut bridge,
            &mut seq,
            ask(
                "h",
                DeviceAsk::State {
                    device_id: "hyd-1".into(),
                },
            ),
            T0,
        );
        let answers = round(&mut bridge, &cloud, T0);
        assert_eq!(
            answers[0],
            DevicesAnswer {
                req_id: "d".into(),
                answer: DeviceAnswer::State {
                    device_id: "desk-1".into(),
                    status: DeviceStatus::Ok,
                    state: DeviceState {
                        online: Some(false),
                        ..DeviceState::default()
                    },
                }
            }
        );
        assert_eq!(
            answers[1].answer,
            DeviceAnswer::State {
                device_id: "hyd-1".into(),
                status: DeviceStatus::Ok,
                state: DeviceState {
                    online: Some(true),
                    height_cm: None,
                    today_ml: Some(1200),
                    sips_today: Some(5),
                    goal_ml: Some(2000),
                }
            }
        );
    }

    /// One test per answer (the first builder's rule 4): ok, refused and
    /// error, each with the `req_id` it was asked by.
    #[test]
    fn a_command_answers_ok_refused_and_error_under_its_req_id() {
        for (word, reason, status) in [
            ("ok", None, DeviceStatus::Ok),
            ("refused", Some("not_allowed"), DeviceStatus::Refused),
            ("error", Some("invalid_value"), DeviceStatus::Error),
            ("offline", None, DeviceStatus::Error),
        ] {
            let cloud = FakeCloud {
                command: word,
                reason,
            };
            let mut seq = 0;
            let mut bridge = listed(&cloud, &mut seq);
            voice(&mut bridge, &mut seq, goto("m1", "desk-1", 110), T0);
            let sent = bridge.due(T0, true);
            assert_eq!(sent[0].kind, COMMAND);
            assert_eq!(
                sent[0].data,
                json!({"req_id": "m1", "device_id": "desk-1", "command": "goto_height",
                       "data": {"value": "110"}})
            );
            let (kind, data) = cloud.reply(&sent[0]);
            bridge.down(kind, &data, T0);
            assert_eq!(
                bridge.drain().answers,
                vec![DevicesAnswer {
                    req_id: "m1".into(),
                    answer: DeviceAnswer::Command {
                        device_id: "desk-1".into(),
                        status,
                        reason: reason.map(str::to_owned),
                    }
                }],
                "{word}"
            );
        }
    }

    #[test]
    fn a_device_the_cloud_did_not_list_is_never_sent_to() {
        let mut seq = 0;
        let mut bridge = Bridge::default();
        // Nothing listed yet: even a real desk's id is not sent to.
        voice(&mut bridge, &mut seq, goto("a", "desk-1", 100), T0);
        assert!(bridge.due(T0, true).is_empty());
        let mut bridge = listed(&FakeCloud::ok(), &mut seq);
        // An invented id, and a height asked of a bottle.
        voice(&mut bridge, &mut seq, goto("b", "desk-9", 100), T0);
        voice(&mut bridge, &mut seq, goto("c", "hyd-1", 100), T0);
        assert!(bridge.due(T0, true).is_empty());
        let answers = bridge.drain().answers;
        assert_eq!(answers.len(), 2);
        assert!(answers.iter().all(|a| a.answer
            == DeviceAnswer::None {
                why: NoAnswer::Unlisted
            }));
    }

    #[test]
    fn a_height_off_the_desks_range_is_not_even_taken() {
        let mut seq = 0;
        let mut bridge = listed(&FakeCloud::ok(), &mut seq);
        for cm in [0, 59, 133] {
            assert_eq!(
                voice(&mut bridge, &mut seq, goto("x", "desk-1", cm), T0),
                Taken::Invalid
            );
        }
        assert!(bridge.due(T0, true).is_empty());
    }

    #[test]
    fn only_the_voices_asks_are_taken_and_the_journal_names_another_once() {
        let mut bridge = Bridge::default();
        let list = || ask("l", DeviceAsk::List);
        assert_eq!(
            bridge.take(("test-tool", "b", 1), list(), T0),
            Taken::NotVoice(Some("test-tool".into()))
        );
        assert_eq!(
            bridge.take(("test-tool", "b", 2), list(), T0),
            Taken::NotVoice(None)
        );
        assert!(
            dropped_line(&Taken::NotVoice(Some("test-tool".into())))
                .unwrap()
                .contains("test-tool")
        );
        assert_eq!(dropped_line(&Taken::NotVoice(None)), None);
        assert!(bridge.due(T0, true).is_empty());
    }

    #[test]
    fn an_ask_heard_again_or_with_a_req_id_in_use_is_sent_once() {
        let mut bridge = Bridge::default();
        let list = |id: &str| ask(id, DeviceAsk::List);
        assert_eq!(bridge.take((VOICE, "b", 5), list("a"), T0), Taken::Kept);
        assert_eq!(bridge.take((VOICE, "b", 5), list("b"), T0), Taken::Again);
        assert_eq!(
            bridge.take((VOICE, "b", 6), list("a"), T0),
            Taken::Duplicate
        );
        assert_eq!(bridge.due(T0, true).len(), 1);
        // A new boot starts its `seq` again.
        assert_eq!(bridge.take((VOICE, "b2", 1), list("c"), T0), Taken::Kept);
    }

    #[test]
    fn no_answer_in_time_is_a_timeout_and_a_late_reply_is_dropped() {
        let cloud = FakeCloud::ok();
        let mut seq = 0;
        let mut bridge = listed(&cloud, &mut seq);
        voice(&mut bridge, &mut seq, goto("m", "desk-1", 100), T0);
        let sent = bridge.due(T0, true);
        assert!(bridge.due(T0 + ANSWER_WITHIN_MS - 1, true).is_empty());
        assert!(bridge.drain().answers.is_empty());
        bridge.due(T0 + ANSWER_WITHIN_MS, true);
        assert_eq!(
            bridge.drain().answers,
            vec![DevicesAnswer {
                req_id: "m".into(),
                answer: DeviceAnswer::None {
                    why: NoAnswer::Timeout
                }
            }]
        );
        let (kind, data) = cloud.reply(&sent[0]);
        bridge.down(kind, &data, T0 + ANSWER_WITHIN_MS + 1);
        let drained = bridge.drain();
        assert!(drained.answers.is_empty(), "never two answers to one ask");
        assert_eq!(bridge.pending(), 0);
    }

    #[test]
    fn with_the_broker_away_nothing_is_kept_and_nothing_is_sent_later() {
        let mut seq = 0;
        let mut bridge = listed(&FakeCloud::ok(), &mut seq);
        voice(&mut bridge, &mut seq, goto("m", "desk-1", 100), T0);
        assert!(bridge.due(T0, false).is_empty());
        assert_eq!(
            bridge.drain().answers,
            vec![DevicesAnswer {
                req_id: "m".into(),
                answer: DeviceAnswer::None {
                    why: NoAnswer::Away
                }
            }]
        );
        // The broker is back: the move that was refused is not sent now.
        assert!(bridge.due(T0 + 5_000, true).is_empty());
    }

    #[test]
    fn a_reply_that_matches_no_ask_is_dropped_whatever_it_says() {
        let cloud = FakeCloud::ok();
        let mut seq = 0;
        let mut bridge = listed(&cloud, &mut seq);
        voice(&mut bridge, &mut seq, goto("m", "desk-1", 100), T0);
        bridge.due(T0, true);
        // Another req_id; no req_id; the right req_id with the wrong kind;
        // the right kind for another device.
        for (kind, data) in [
            (
                COMMAND_RESULT,
                json!({"req_id": "zzz", "device_id": "desk-1", "status": "ok"}),
            ),
            (
                COMMAND_RESULT,
                json!({"device_id": "desk-1", "status": "ok"}),
            ),
            (
                STATE,
                json!({"req_id": "m", "device_id": "desk-1", "status": "ok", "state": {}}),
            ),
            (
                COMMAND_RESULT,
                json!({"req_id": "m", "device_id": "hyd-1", "status": "ok"}),
            ),
        ] {
            bridge.down(kind, &data, T0);
        }
        let drained = bridge.drain();
        assert!(drained.answers.is_empty());
        assert_eq!(
            drained
                .lines
                .iter()
                .filter(|l| l.contains("matched no ask"))
                .count(),
            1
        );
        assert_eq!(
            bridge.pending(),
            1,
            "the ask is still waiting for its own answer"
        );
    }

    /// The first builder's rule 3, and the latency case: a stop is sent
    /// first, past every cap, and nothing waiting moves the desk after it.
    #[test]
    fn a_stop_goes_first_past_the_caps_and_cancels_the_moves_still_waiting() {
        let mut seq = 0;
        let mut bridge = listed(&FakeCloud::ok(), &mut seq);
        voice(&mut bridge, &mut seq, goto("m1", "desk-1", 110), T0);
        voice(
            &mut bridge,
            &mut seq,
            ask(
                "s",
                DeviceAsk::State {
                    device_id: "hyd-1".into(),
                },
            ),
            T0,
        );
        voice(&mut bridge, &mut seq, stop("st", "desk-1"), T0);
        let sent = bridge.due(T0, true);
        let kinds: Vec<_> = sent
            .iter()
            .map(|s| (s.kind, s.data["req_id"].clone()))
            .collect();
        assert_eq!(kinds, vec![(COMMAND, json!("st")), (STATE_GET, json!("s"))]);
        assert_eq!(
            sent[0].data,
            json!({"req_id": "st", "device_id": "desk-1", "command": "stop"})
        );
        assert_eq!(
            bridge.drain().answers,
            vec![DevicesAnswer {
                req_id: "m1".into(),
                answer: DeviceAnswer::None {
                    why: NoAnswer::Cancelled
                }
            }]
        );
    }

    #[test]
    fn a_stop_is_sent_with_the_rate_and_the_pending_caps_full_and_for_a_device_not_listed() {
        let mut seq = 0;
        let mut bridge = Bridge::default();
        // Fill the asks a minute, and the ones out.
        for n in 0..PER_MINUTE {
            voice(
                &mut bridge,
                &mut seq,
                ask(&format!("l{n}"), DeviceAsk::List),
                T0,
            );
            if n % MOST_PENDING == MOST_PENDING - 1 {
                bridge.due(T0, true);
            }
        }
        bridge.drain();
        // Not listed (a lamp-cloud restart lost the list while the desk moved).
        voice(&mut bridge, &mut seq, stop("st", "desk-1"), T0);
        let sent = bridge.due(T0, true);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].data["command"], "stop");
        assert!(bridge.drain().answers.is_empty());
    }

    #[test]
    fn too_many_asks_are_answered_busy_and_not_sent() {
        let mut seq = 0;
        let mut bridge = Bridge::default();
        for n in 0..MOST_PENDING + 2 {
            voice(
                &mut bridge,
                &mut seq,
                ask(&format!("l{n}"), DeviceAsk::List),
                T0,
            );
        }
        assert_eq!(bridge.due(T0, true).len(), MOST_PENDING);
        let busy = bridge.drain().answers;
        assert_eq!(busy.len(), 2);
        assert!(busy.iter().all(|a| a.answer
            == DeviceAnswer::None {
                why: NoAnswer::Busy
            }));
        // A minute on, and answered, there is room again.
        bridge.due(T0 + MINUTE_MS, true);
        bridge.drain();
        voice(
            &mut bridge,
            &mut seq,
            ask("again", DeviceAsk::List),
            T0 + MINUTE_MS,
        );
        assert_eq!(bridge.due(T0 + MINUTE_MS, true).len(), 1);
    }

    #[test]
    fn the_hydrates_news_is_passed_down_small_and_only_for_kinds_the_lamp_knows() {
        let mut bridge = Bridge::default();
        let sip = json!({"device_id": "hyd-1", "device_type": "hydrate", "event": "hydrate.sip",
                         "at": "2026-11-18T03:00:00Z",
                         "data": {"ml": 30, "today_ml": 250, "sips_today": 6}});
        bridge.down(EVENT, &sip, T0);
        let mut other = sip.clone();
        other["device_type"] = "toaster".into();
        bridge.down(EVENT, &other, T0);
        let mut odd = sip.clone();
        odd["event"] = "a b".into();
        bridge.down(EVENT, &odd, T0);
        let drained = bridge.drain();
        assert_eq!(drained.events.len(), 1);
        assert_eq!(drained.events[0].event, "hydrate.sip");
        assert_eq!(drained.events[0].data["today_ml"], 250);
        for n in 0..EVENTS_PER_MINUTE + 5 {
            bridge.down(EVENT, &sip, T0 + n as u64);
        }
        assert_eq!(bridge.drain().events.len(), EVENTS_PER_MINUTE - 1);
    }

    #[test]
    fn off_takes_and_sends_nothing() {
        for value in [
            None,
            Some(""),
            Some("off"),
            Some("1"),
            Some("ON"),
            Some("true"),
        ] {
            assert!(!on_from(value), "{value:?}");
        }
        assert!(on_from(Some("on")));
        assert!(is_down(LIST) && is_down(EVENT) && !is_down("devices.command"));
    }
}
