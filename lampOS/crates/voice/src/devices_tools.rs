//! The owner's desk and Hydrate bottles, read by voice (#7470 S2): "what
//! height is my desk", "how much have I drunk today". Two functions the
//! model calls with no arguments, declared only with `LAMP_DEVICES=on` in the
//! voice's unit; never a skill. Reads only: nothing here moves a desk (the
//! move is S3, behind the same switch).
//!
//! The voice asks `lamp-cloud` on the bus (`devices/ask`, a list and then a
//! state of each device it needs) and waits for the answers on
//! `devices/answer`, matched by `req_id`, within [`ANSWER_WITHIN`] for the
//! whole call, one deadline shared by every round. The list is kept
//! [`LIST_KEPT`] so a repeated read is one round trip, and dropped when
//! `lamp-cloud` says a device is not one its list gave (it restarted). The
//! lamp acts only on a device the cloud's list gave, never one taken from
//! speech: these functions take no argument.
//!
//! What the model is told is a fact and a `say` in a few English words, to
//! be said in the person's language, as `reports_stop` does: the numbers
//! change each time, so no line here is a fixed line. A reading the cloud
//! has not got (a desk's height before it reports) is said as not known,
//! never made up.
use lamp_gemini::Tool;
use lamp_msgs::{
    DeviceAnswer, DeviceAsk, DeviceCommand, DeviceRow, DeviceState, DeviceStatus, DeviceType,
    DevicesAnswer, DevicesAsk, NoAnswer,
};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

/// `on` for the functions to be declared. Anything else: never.
pub const ENVIRONMENT: &str = "LAMP_DEVICES";
/// The provider worker's flag for it.
pub const FLAG: &str = "--devices";
/// The desk's height.
pub const DESK: &str = "desk_height";
/// How much the person has drunk today.
pub const WATER: &str = "water_today";
/// Stops the desk (#7470 S3a): no argument, no yes, from anyone.
pub const STOP: &str = "desk_stop";
/// How long a stop waits for the cloud's word on it.
pub const STOP_WITHIN: Duration = Duration::from_secs(3);
/// How long a guard watches for a touch after a move: a desk goes a few
/// centimetres a second over at most 72 cm.
pub const GUARD_FOR: Duration = Duration::from_secs(60);
/// How long a whole call waits for the cloud, every round together (the
/// list, then the state): the cloud usually answers in about a second, and
/// `lamp-cloud` itself gives it 8 s, so the person is never made to wait
/// that long; a late answer is dropped by its `req_id`.
pub const ANSWER_WITHIN: Duration = Duration::from_secs(5);
/// How long the cloud's list is kept.
pub const LIST_KEPT: Duration = Duration::from_secs(600);
/// The most bottles read at once (the cloud may list several).
const MOST_BOTTLES: usize = 4;
/// The voice's name on the bus for its asks (`lamp-cloud` takes only it).
const SOURCE: &str = "voice";

/// What the person hears, in English words the model says in their language.
pub const NO_DESK: &str = "I can't find a desk of yours.";
pub const NO_BOTTLE: &str = "I can't find a water bottle of yours.";
pub const HEIGHT_UNKNOWN: &str = "I don't know the desk's height yet.";
pub const DESK_OFFLINE: &str = "The desk isn't answering. It may be switched off.";
pub const DESK_UNREACHED: &str = "I couldn't reach the desk.";
pub const BOTTLE_UNREACHED: &str = "I couldn't reach your water bottle.";
pub const WATER_UNKNOWN: &str = "I don't know how much you've had yet.";
pub const AWAY: &str = "I'm not online right now, so I can't reach your devices.";

/// The provider worker's flag, with the switch `on`.
pub fn arguments(value: Option<&str>) -> Vec<String> {
    if value == Some("on") {
        vec![FLAG.to_owned()]
    } else {
        Vec::new()
    }
}

/// What a call reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Read {
    Desk,
    Water,
}

/// What `name` reads, if it is one of these functions.
pub fn read_of(name: &str) -> Option<Read> {
    match name {
        DESK => Some(Read::Desk),
        WATER => Some(Read::Water),
        _ => None,
    }
}

pub fn is_tool(name: &str) -> bool {
    read_of(name).is_some() || is_stop_tool(name)
}

pub fn is_stop_tool(name: &str) -> bool {
    name == STOP
}

/// The functions: the reads, asked about only when the person asks, and
/// the stop.
pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: STOP.into(),
            description: "Call this AT ONCE when the person says stop, or asks you to stop \
                          the standing desk, while it is moving. It needs no yes and no \
                          reason; say what it gives you as \"say\", in a few words, in their \
                          language. It only stops the desk."
                .into(),
            parameters: None,
            silent: false,
        },
        Tool {
            name: DESK.into(),
            description: "Call this only when the person asks how high their standing desk is. \
                          It reads the desk from the cloud. Then say what it gives you as \
                          \"say\", in a few words, in their language. It cannot move the desk. \
                          Never bring the desk up unless you were asked."
                .into(),
            parameters: None,
            silent: false,
        },
        Tool {
            name: WATER.into(),
            description: "Call this only when the person asks how much water they have drunk \
                          today, from their smart water bottle. Then say what it gives you as \
                          \"say\", in a few words, in their language. Never remark on their \
                          drinking unless you were asked."
                .into(),
            parameters: None,
            silent: false,
        },
    ]
}

/// What the bus gives these functions: asks sent together, and their
/// answers by `req_id`.
pub trait Link {
    /// Sends every ask, and waits until each has an answer or `within` has
    /// passed. The answers that came, in any order.
    fn ask(&mut self, asks: Vec<DevicesAsk>, within: Duration) -> Vec<DevicesAnswer>;
}

/// The lamp's bus.
pub struct Bus(pub Option<lamp_bus::Bus>);

impl Bus {
    /// The lamp's bus (`LAMP_BUS_ROOT`, or its default), the call's own.
    pub fn of_the_lamp() -> Self {
        Self(lamp_bus::Bus::from_environment().ok())
    }
}

impl Link for Bus {
    fn ask(&mut self, asks: Vec<DevicesAsk>, within: Duration) -> Vec<DevicesAnswer> {
        let Some(bus) = self.0.as_ref() else {
            return Vec::new();
        };
        // Listening before asking: an answer is never missed.
        let Ok(mut heard) = bus.subscribe_typed(DevicesAnswer::TOPIC, SOURCE, DevicesAnswer::TYPE)
        else {
            return Vec::new();
        };
        let Ok(mut out) =
            bus.publisher_declared(DevicesAsk::TOPIC, &DevicesAsk::declaration(), SOURCE)
        else {
            return Vec::new();
        };
        let mut waiting: Vec<String> = Vec::new();
        for ask in &asks {
            if out.publish_json(lamp_ipc::monotonic_us(), ask).is_ok() {
                waiting.push(ask.req_id.clone());
            }
        }
        let began = Instant::now();
        let mut answers = Vec::new();
        while !waiting.is_empty() {
            let Some(left) = within.checked_sub(began.elapsed()) else {
                break;
            };
            match heard.recv(left) {
                Ok(Some(message)) => {
                    if let Ok(answer) = message.parse::<DevicesAnswer>()
                        && let Some(at) = waiting.iter().position(|id| *id == answer.req_id)
                    {
                        waiting.swap_remove(at);
                        answers.push(answer);
                    }
                }
                Ok(None) => {}
                Err(_) => break,
            }
        }
        answers
    }
}

/// A `req_id` no other call of this process has.
fn req_id() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    format!(
        "v{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}

/// The cloud's list, as last read.
#[derive(Clone, Debug, Default)]
pub struct Cache {
    rows: Option<(Instant, Vec<DeviceRow>)>,
}

impl Cache {
    fn fresh(&self, now: Instant) -> Option<&[DeviceRow]> {
        self.rows
            .as_ref()
            .filter(|(at, _)| now.duration_since(*at) < LIST_KEPT)
            .map(|(_, rows)| rows.as_slice())
    }
}

/// The lamp's own cache, one per process.
pub static CACHE: std::sync::Mutex<Cache> = std::sync::Mutex::new(Cache { rows: None });

/// One call: the list (kept a while), then the state of the device or
/// bottles it needs, asked together. `(what the model is told, whether it
/// was read)`.
pub fn run(link: &mut impl Link, cache: &mut Cache, read: Read, within: Duration) -> (Value, bool) {
    let want = match read {
        Read::Desk => DeviceType::Desk,
        Read::Water => DeviceType::Hydrate,
    };
    // One deadline for the whole call, every round in it.
    let deadline = Instant::now() + within;
    let left = || deadline.saturating_duration_since(Instant::now());
    // The kept list is tried first; a device `lamp-cloud` does not know
    // (it restarted), or a kept list with nothing of the wanted kind (the
    // desk may have been paired since), drops it and the read goes again
    // from a fresh list.
    for kept in [true, false] {
        let from_cache = kept && cache.fresh(Instant::now()).is_some();
        let rows: Vec<DeviceRow> = match cache.fresh(Instant::now()).filter(|_| kept) {
            Some(rows) => rows.to_vec(),
            None => match list(link, left()) {
                Ok(rows) => {
                    cache.rows = Some((Instant::now(), rows.clone()));
                    rows
                }
                Err(told) => return (told, false),
            },
        };
        let devices: Vec<&DeviceRow> = rows
            .iter()
            .filter(|row| row.device_type == want)
            .take(if read == Read::Desk { 1 } else { MOST_BOTTLES })
            .collect();
        if devices.is_empty() {
            if from_cache {
                cache.rows = None;
                continue;
            }
            let say = if read == Read::Desk {
                NO_DESK
            } else {
                NO_BOTTLE
            };
            return (json!({"ok": true, "say": say}), true);
        }
        let asks = devices
            .iter()
            .map(|device| DevicesAsk {
                req_id: req_id(),
                ask: DeviceAsk::State {
                    device_id: device.device_id.clone(),
                },
            })
            .collect::<Vec<_>>();
        let answers = link.ask(asks, left());
        if answers.iter().any(|answer| {
            matches!(
                answer.answer,
                DeviceAnswer::None {
                    why: NoAnswer::Unlisted
                }
            )
        }) && kept
        {
            cache.rows = None;
            continue;
        }
        let states: Vec<(DeviceStatus, DeviceState)> = answers
            .into_iter()
            .filter_map(|answer| match answer.answer {
                DeviceAnswer::State { status, state, .. } => Some((status, state)),
                _ => None,
            })
            .collect();
        return told(read, &states);
    }
    unreachable!("the second pass always returns")
}

fn list(link: &mut impl Link, within: Duration) -> Result<Vec<DeviceRow>, Value> {
    let ask = DevicesAsk {
        req_id: req_id(),
        ask: DeviceAsk::List,
    };
    let id = ask.req_id.clone();
    let answers = link.ask(vec![ask], within);
    match answers.into_iter().find(|answer| answer.req_id == id) {
        Some(DevicesAnswer {
            answer: DeviceAnswer::List { devices },
            ..
        }) => Ok(devices),
        Some(DevicesAnswer {
            answer: DeviceAnswer::None { why },
            ..
        }) => Err(
            json!({"error": if why == NoAnswer::Away { AWAY } else { "I couldn't reach your devices right now." }}),
        ),
        _ => Err(json!({"error": "I couldn't reach your devices right now."})),
    }
}

/// What the model is told of the states read.
fn told(read: Read, states: &[(DeviceStatus, DeviceState)]) -> (Value, bool) {
    match read {
        Read::Desk => {
            let Some((status, state)) = states.first() else {
                return (json!({"error": DESK_UNREACHED}), false);
            };
            if *status != DeviceStatus::Ok {
                return (json!({"error": DESK_UNREACHED}), false);
            }
            match state.height_cm {
                Some(cm) => (
                    json!({"ok": true, "height_cm": cm, "say": format!("Your desk is at {cm} centimetres.")}),
                    true,
                ),
                None if state.online == Some(false) => (
                    json!({"ok": true, "online": false, "say": DESK_OFFLINE}),
                    true,
                ),
                None => (json!({"ok": true, "say": HEIGHT_UNKNOWN}), true),
            }
        }
        Read::Water => {
            let read: Vec<&DeviceState> = states
                .iter()
                .filter(|(status, _)| *status == DeviceStatus::Ok)
                .map(|(_, state)| state)
                .collect();
            if read.is_empty() {
                return (json!({"error": BOTTLE_UNREACHED}), false);
            }
            // Several bottles: the one drunk from most today, the person's
            // own, not a sum that would count a bottle left in a drawer.
            let best = read
                .iter()
                .filter_map(|state| state.today_ml.map(|ml| (ml, *state)))
                .max_by_key(|(ml, _)| *ml);
            let Some((ml, state)) = best else {
                return (json!({"ok": true, "say": WATER_UNKNOWN}), true);
            };
            let mut say = format!("You've had {} today", volume(ml));
            if let Some(goal) = state.goal_ml.filter(|goal| *goal > 0) {
                say.push_str(&format!(", of {}", volume(goal)));
            }
            say.push('.');
            (
                json!({"ok": true, "today_ml": ml, "goal_ml": state.goal_ml,
                       "sips_today": state.sips_today, "say": say}),
                true,
            )
        }
    }
}

/// Millilitres as a person says them: "250 millilitres", "1.2 litres".
fn volume(ml: u32) -> String {
    if ml < 1000 {
        format!("{ml} millilitres")
    } else {
        let tenths = (ml + 50) / 100;
        let (whole, tenth) = (tenths / 10, tenths % 10);
        let litres = if tenth == 0 {
            whole.to_string()
        } else {
            format!("{whole}.{tenth}")
        };
        format!("{litres} {}", if tenths == 10 { "litre" } else { "litres" })
    }
}

/// What the person hears of a stop.
pub const STOPPED: &str = "Okay, I've told the desk to stop.";
pub const STOP_REFUSED: &str = "I can't stop that desk.";
pub const STOP_NOT_SURE: &str = "I couldn't reach the desk, so I'm not sure it stopped.";

/// A desk move in flight: the desk, and until when a touch stops it. Set
/// after a move's `ok` (S3b), cleared by a stop.
#[derive(Clone, Debug)]
pub struct Moving {
    pub device_id: String,
    pub until: Instant,
}

static MOVING: std::sync::Mutex<Option<Moving>> = std::sync::Mutex::new(None);

/// The move in flight, if any and not past its time.
pub fn moving_now() -> Option<Moving> {
    MOVING
        .lock()
        .ok()
        .and_then(|moving| moving.clone())
        .filter(|moving| Instant::now() < moving.until)
}

fn set_moving(moving: Option<Moving>) {
    if let Ok(mut kept) = MOVING.lock() {
        *kept = moving;
    }
}

/// A stop: no yes, no presence, from anyone, asked at once. The desk is the
/// one in flight, else the list's first desk (the kept list: no round trip).
/// Told as it was sent, with the cloud's word if it comes within `within`:
/// `ok` is that the stop was handed to the desk, any other end is "not sure".
pub fn run_stop(link: &mut impl Link, cache: &mut Cache, within: Duration) -> (Value, bool) {
    let deadline = Instant::now() + within;
    let left = || deadline.saturating_duration_since(Instant::now());
    let device_id = match moving_now() {
        Some(moving) => moving.device_id,
        None => {
            let rows: Vec<DeviceRow> = match cache.fresh(Instant::now()) {
                Some(rows) => rows.to_vec(),
                None => match list(link, left()) {
                    Ok(rows) => {
                        cache.rows = Some((Instant::now(), rows.clone()));
                        rows
                    }
                    Err(told) => return (told, false),
                },
            };
            match rows.iter().find(|row| row.device_type == DeviceType::Desk) {
                Some(desk) => desk.device_id.clone(),
                None => return (json!({"ok": true, "say": NO_DESK}), true),
            }
        }
    };
    // Whatever comes of the answer, a desk told to stop is no move in flight.
    set_moving(None);
    let ask = DevicesAsk {
        req_id: req_id(),
        ask: DeviceAsk::Command {
            device_id: device_id.clone(),
            command: DeviceCommand::Stop,
        },
    };
    let id = ask.req_id.clone();
    let answers = link.ask(vec![ask], left());
    match answers.into_iter().find(|answer| answer.req_id == id) {
        Some(DevicesAnswer {
            answer: DeviceAnswer::Command { status, .. },
            ..
        }) => match status {
            DeviceStatus::Ok => (json!({"ok": true, "say": STOPPED}), true),
            DeviceStatus::Refused => (json!({"error": STOP_REFUSED}), false),
            DeviceStatus::Error => (json!({"error": STOP_NOT_SURE}), false),
        },
        Some(DevicesAnswer {
            answer: DeviceAnswer::None {
                why: NoAnswer::Away,
            },
            ..
        }) => (json!({"error": AWAY}), false),
        _ => (json!({"error": STOP_NOT_SURE}), false),
    }
}

/// Arms the touch guard on the lamp's own bus for a move of `device_id` that
/// is in flight until `until`: a touch of the lamp stops the desk (S3b calls
/// this when a move's command goes).
pub fn arm_guard(device_id: String, until: Instant) {
    if let Ok(bus) = lamp_bus::Bus::from_environment() {
        let _ = arm_guard_on(&bus, device_id, until);
    }
}

/// [`arm_guard`] on `bus`: the touches are listened for and the stop's
/// publisher made BEFORE this returns, so a touch right after is never
/// missed; the watching is a thread of its own, joined by tests. A touch
/// event (any contact, any source: the lamp's head, its button) from
/// Controls, enabled, fresh and valid, sends `Stop` at once, once, and ends
/// the guard. None if the bus cannot be listened to: then the move must not go.
pub fn arm_guard_on(
    bus: &lamp_bus::Bus,
    device_id: String,
    until: Instant,
) -> Option<std::thread::JoinHandle<()>> {
    let mut touches = bus
        .subscribe_typed(
            lamp_msgs::TouchEvent::TOPIC,
            SOURCE,
            lamp_msgs::TouchEvent::TYPE,
        )
        .ok()?;
    let mut out = bus
        .publisher_declared(DevicesAsk::TOPIC, &DevicesAsk::declaration(), SOURCE)
        .ok()?;
    let armed_us = lamp_ipc::monotonic_us();
    set_moving(Some(Moving {
        device_id: device_id.clone(),
        until,
    }));
    Some(std::thread::spawn(move || {
        while Instant::now() < until {
            // Stopped by a spoken or model stop: the guard is no more.
            if moving_now().is_none_or(|moving| moving.device_id != device_id) {
                return;
            }
            let Ok(Some(message)) = touches.recv(Duration::from_millis(20)) else {
                continue;
            };
            let Ok(event) = message.parse::<lamp_msgs::TouchEvent>() else {
                continue;
            };
            if message.expired
                || message.header.source != "controls"
                || !event.enabled
                || event.check().is_err()
                || event.observed_us < armed_us
            {
                continue;
            }
            let stop = DevicesAsk {
                req_id: req_id(),
                ask: DeviceAsk::Command {
                    device_id: device_id.clone(),
                    command: DeviceCommand::Stop,
                },
            };
            set_moving(None);
            if out.publish_json(lamp_ipc::monotonic_us(), &stop).is_err() {
                eprintln!("lamp-voice devices: a touch could not stop the desk on the bus");
            } else {
                eprintln!("lamp-voice devices: a touch stopped the desk");
            }
            return;
        }
    }))
}

#[cfg(test)]
mod tests;
