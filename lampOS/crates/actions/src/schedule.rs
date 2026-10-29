//! Opt-in model tools for Schedule. One validated call gets one bounded attempt;
//! a lost mutation reply is unknown, never grounds for a new-guard retry.
use crate::Function;
use lamp_schedule::{
    client,
    protocol::{Guard, Request},
    time::{Deadline, Fold, MAX_AFTER_SECONDS, Repeat, Weekday, When, validate_zone},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    ffi::OsStr,
    io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub const DEFAULT_SOCKET: &str = "/run/lampos-schedule/call.sock";
pub const DEADLINE: Duration = Duration::from_millis(1250);
/// Told beside a list asked `pending_only` that left out items set but not
/// yet due (#1595).
pub const NOT_YET_DUE: &str = "Timers or reminders are set that are not due yet, so they are not in this list. List again without pending_only before saying none are set, or to find one to cancel.";

/// Voice reserves 16 bytes in its provider's 4096-byte tool-result allowance.
pub const MAX_RESULT_BYTES: usize = 4096 - 16;
pub const NAMES: [&str; 6] = [
    "set_timer",
    "set_reminder",
    "repeat_reminder",
    "list_reminders",
    "cancel_reminder",
    "snooze_reminder",
];

pub fn is_tool(name: &str) -> bool {
    NAMES.contains(&name)
}

/// The existing provider's small Schema vocabulary. Runtime validation below
/// is stricter than a model schema; byte limits and unknown fields are checked.
pub fn catalog() -> Vec<Function> {
    let text = json!({"type":"string","description":"A short label: 1 to 300 UTF-8 bytes, without control characters."});
    let seconds = json!({"type":"integer","minimum":1,"maximum":MAX_AFTER_SECONDS});
    let time =
        json!({"type":"string","description":"Local time in exact HH:MM, 00:00 through 23:59."});
    let zone = json!({"type":"string","description":"An IANA time-zone name, only if specified; otherwise use the lamp's configured zone."});
    let id = json!({"type":"integer","minimum":1,"description":"An exact ID from list_reminders; never guess one."});
    let function = |name, description, properties, required| Function {
        name,
        description,
        parameters: Some(json!({"type":"object","properties":properties,"required":required})),
    };
    vec![
        function(
            NAMES[0],
            "Set a timer or a relative reminder only when asked. For a duration from now, such as 'in two minutes', 'in ten minutes', or 'in an hour', use this tool with the requested label and whole seconds (120, 600, or 3600 respectively); never round to a clock time. Confirm the returned time only after durable success. If the outcome is unknown, list current reminders before asking again.",
            json!({"text":text,"seconds":seconds}),
            json!(["text", "seconds"]),
        ),
        function(
            NAMES[1],
            "Set one reminder at a requested local clock time, such as 'at nine tomorrow morning'. For relative times from now, use set_timer with the requested label and whole seconds instead; do not round to HH:MM. Ask for missing or ambiguous intent; never calculate a Unix timestamp.",
            json!({"text":text,"time":time,"day":{"type":"string","description":"today, tomorrow, next, or exact YYYY-MM-DD; omitted means the next strictly future occurrence."},"zone":zone,"fold":{"type":"string","enum":["earlier","later","reject"]}}),
            json!(["text", "time"]),
        ),
        function(
            NAMES[2],
            "Repeat a reminder daily or weekly at the requested local time. Weekly needs a weekday; daily must omit it.",
            json!({"text":text,"time":time,"every":{"type":"string","enum":["daily","weekly"]},"weekday":{"type":"string","enum":["monday","tuesday","wednesday","thursday","friday","saturday","sunday"]},"zone":zone}),
            json!(["text", "time", "every"]),
        ),
        function(
            NAMES[3],
            "Read one bounded page of active timers and reminders, oldest first. Each item's kind is timer (set with set_timer) or reminder. To say what is set, or to find one to cancel, leave pending_only out. If next_after_id is present, use it as after_id to read more; never describe an incomplete page as the whole list. If not_yet_due, how many are set and not yet due in all, is more than 0, timers or reminders are set: list again without pending_only before saying none are.",
            json!({"pending_only":{"type":"boolean","description":"true lists only items that are due now and not yet said; leave it out to list everything that is set."},"after_id":{"type":"integer","minimum":0}}),
            json!([]),
        ),
        function(
            NAMES[4],
            "Cancel the selected timer or whole reminder series only when asked. Use an ID from the current list, read without pending_only. For a timer, cancel only an item whose kind is timer; for a reminder, only one whose kind is reminder.",
            json!({"id":id}),
            json!(["id"]),
        ),
        function(
            NAMES[5],
            "Postpone the selected currently due occurrence by whole seconds. This preserves a repeating reminder's original schedule. Use an ID from the current list.",
            json!({"id":id,"seconds":seconds}),
            json!(["id", "seconds"]),
        ),
    ]
}

#[derive(Clone, Debug)]
pub struct Adapter {
    socket: PathBuf,
    /// [`DEADLINE`] for every product path.
    deadline: Duration,
}

impl Adapter {
    pub fn new(socket: PathBuf) -> io::Result<Self> {
        client::validate_socket(&socket).map_err(io::Error::other)?;
        Ok(Self {
            socket,
            deadline: DEADLINE,
        })
    }

    /// The same adapter, giving one call `deadline` instead of [`DEADLINE`]:
    /// for a host test against a real Schedule service on a loaded machine,
    /// where one durable commit can take past 1250 ms (lamp-voice's push
    /// runs on 29 Oct). Never used on a lamp.
    pub fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }

    /// An absent or inexact switch does not even inspect the socket setting.
    pub fn configured(switch: Option<&OsStr>, socket: Option<&Path>) -> io::Result<Option<Self>> {
        if switch != Some(OsStr::new("on")) {
            return Ok(None);
        }
        Self::new(
            socket
                .unwrap_or_else(|| Path::new(DEFAULT_SOCKET))
                .to_owned(),
        )
        .map(Some)
    }

    pub fn from_environment() -> io::Result<Option<Self>> {
        let switch = std::env::var_os("LAMP_SCHEDULE_TOOLS");
        if switch.as_deref() != Some(OsStr::new("on")) {
            return Ok(None);
        }
        let socket = std::env::var_os("LAMP_SCHEDULE_SOCKET");
        Self::configured(switch.as_deref(), socket.as_deref().map(Path::new))
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Returns the ordinary Actions envelope. Successful result objects fit
    /// Voice's separate model-result budget. Failure sentences are fixed and
    /// never contain the caller's reminder text or an owner's arbitrary error.
    pub fn call(&self, name: &str, args: &Value) -> Value {
        let deadline = Instant::now() + self.deadline;
        match parse(name, args).and_then(|call| self.perform(call, deadline)) {
            Ok(result) if result.to_string().len() <= MAX_RESULT_BYTES => {
                json!({"ok":true,"result":result})
            }
            Ok(_) => failure(Failure::Uncertain),
            Err(why) => failure(why),
        }
    }

    fn ask(&self, request: &Request, deadline: Instant) -> Result<Value, Failure> {
        let mutation = request.guard().is_some();
        let value = serde_json::to_value(request).map_err(|_| Failure::Unavailable)?;
        let reply = client::exchange(&self.socket, &value, deadline).map_err(|e| {
            if mutation && e.may_have_sent {
                Failure::Uncertain
            } else {
                Failure::Unavailable
            }
        })?;
        match reply["ok"].as_bool() {
            Some(true) => {
                identity(&reply).map_err(|_| invalid_reply(mutation))?;
                Ok(reply)
            }
            Some(false) => Err(refused(&reply, mutation)),
            None => Err(invalid_reply(mutation)),
        }
    }

    fn perform(&self, call: Call, deadline: Instant) -> Result<Value, Failure> {
        if let Call::List {
            after_id,
            pending_only,
        } = call
        {
            let reply = self.ask(
                &Request::List {
                    after_id,
                    limit: 8,
                    pending_only,
                },
                deadline,
            )?;
            let mut listed = page(&reply, after_id, pending_only)?;
            if pending_only {
                // #1595: a list of what is due leaves out what is set and not
                // yet due, and "nothing due" read as "nothing set" let a timer
                // the person asked to cancel ring. Said as a count, best
                // effort: the list itself stands if the count can't be read.
                if let Some(waiting) = self
                    .ask(&Request::Status {}, deadline)
                    .ok()
                    .and_then(|status| {
                        Some(
                            status["items"]
                                .as_u64()?
                                .saturating_sub(status["pending"].as_u64()?),
                        )
                    })
                    .filter(|waiting| *waiting > 0)
                {
                    listed["not_yet_due"] = json!(waiting);
                    listed["next"] = json!(NOT_YET_DUE);
                }
            }
            return Ok(listed);
        }
        // Snooze obtains guard and delivery from the SAME Get reply. Other
        // mutations use Status. No conflict or uncertain mutation is retried.
        let before = match &call {
            Call::Snooze { id, .. } => self.ask(&Request::Get { id: *id }, deadline)?,
            _ => self.ask(&Request::Status {}, deadline)?,
        };
        let (store_id, revision) = identity(&before)?;
        let mut random = [0u8; 16];
        getrandom::fill(&mut random).map_err(|_| Failure::Unavailable)?;
        let guard = Guard {
            store_id: store_id.into(),
            revision,
            request_id: random.iter().map(|b| format!("{b:02x}")).collect(),
        };
        let request = match call {
            Call::Set { text, when } => Request::Set { guard, text, when },
            Call::Cancel { id } => Request::Cancel { guard, id },
            Call::Snooze { id, seconds } => {
                let item = item(&before["item"]).map_err(|_| Failure::Unavailable)?;
                if item.id != id {
                    return Err(Failure::Unavailable);
                }
                if item.state != "pending" {
                    return Err(Failure::NotPending);
                }
                Request::Snooze {
                    guard,
                    id,
                    delivery: item.delivery,
                    seconds,
                }
            }
            Call::List { .. } => unreachable!("list returned before mutation"),
        };
        let response = self.ask(&request, deadline)?;
        committed(&request, &response)
    }
}

#[derive(Clone, Copy, Debug)]
enum Failure {
    BadArguments,
    Unavailable,
    Uncertain,
    Conflict,
    NotFound,
    NotPending,
    BadTime,
    Full,
}

fn failure(why: Failure) -> Value {
    let (code, text) = match why {
        Failure::BadArguments => (
            "bad_arguments",
            "I need a valid reminder label, time and exact arguments before I can do that.",
        ),
        Failure::Unavailable => (
            "unavailable",
            "I cannot use my reminder store just now. Please try reading the current reminders later.",
        ),
        Failure::Uncertain => (
            "uncertain",
            "I could not confirm that change. It may have happened. Please list the current reminders before asking me to make the change again.",
        ),
        Failure::Conflict => (
            "conflict",
            "The reminders changed while I was checking. Please list the current reminders before choosing that change again.",
        ),
        Failure::NotFound => ("not_found", "That reminder or timer is no longer active."),
        Failure::NotPending => (
            "not_pending",
            "That exact reminder is not currently due. Please list the current reminders before choosing one to snooze.",
        ),
        Failure::BadTime => (
            "bad_time",
            "I cannot set that time just now. Please clarify the date, time zone or repeated clock time, and check that my clock is ready.",
        ),
        Failure::Full => (
            "full",
            "My reminder store is full. Please list the current reminders and cancel one before adding another.",
        ),
    };
    json!({"ok":false,"error":text,"code":code})
}

fn invalid_reply(mutation: bool) -> Failure {
    if mutation {
        Failure::Uncertain
    } else {
        Failure::Unavailable
    }
}

fn refused(reply: &Value, mutation: bool) -> Failure {
    match reply["error"].as_str() {
        Some("conflict" | "bad_guard") => Failure::Conflict,
        Some("not_found") => Failure::NotFound,
        Some("stale_delivery") => Failure::NotPending,
        Some("bad_time") => Failure::BadTime,
        Some("full") => Failure::Full,
        Some("bad_request" | "bad_text" | "too_long" | "incomplete" | "busy" | "unavailable") => {
            Failure::Unavailable
        }
        // In particular storage errors can occur after commit; timeout or an
        // unrecognized error never proves a mutation did not take effect.
        _ => invalid_reply(mutation),
    }
}

enum Call {
    Set { text: String, when: When },
    List { after_id: u64, pending_only: bool },
    Cancel { id: u64 },
    Snooze { id: u64, seconds: u64 },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Timer {
    text: String,
    seconds: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reminder {
    text: String,
    time: String,
    day: Option<String>,
    zone: Option<String>,
    fold: Option<Fold>,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Every {
    Daily,
    Weekly,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Repeating {
    text: String,
    time: String,
    every: Every,
    weekday: Option<Weekday>,
    zone: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Listing {
    #[serde(default)]
    after_id: u64,
    #[serde(default)]
    pending_only: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Cancel {
    id: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Snooze {
    id: u64,
    seconds: u64,
}

fn decode<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T, Failure> {
    if value
        .as_object()
        .is_none_or(|fields| fields.values().any(Value::is_null))
    {
        return Err(Failure::BadArguments);
    }
    serde_json::from_value(value.clone()).map_err(|_| Failure::BadArguments)
}
fn valid_text(text: &str) -> bool {
    !text.trim().is_empty() && text.len() <= 300 && !text.chars().any(char::is_control)
}
fn seconds(value: u64) -> Result<(), Failure> {
    (1..=MAX_AFTER_SECONDS)
        .contains(&value)
        .then_some(())
        .ok_or(Failure::BadArguments)
}
fn time(value: &str) -> bool {
    let b = value.as_bytes();
    b.len() == 5
        && b[2] == b':'
        && [b[0], b[1], b[3], b[4]].iter().all(u8::is_ascii_digit)
        && &b[..2] <= b"23"
        && &b[3..] <= b"59"
}
fn day(value: &str) -> bool {
    if matches!(value, "today" | "tomorrow" | "next") {
        return true;
    }
    let b = value.as_bytes();
    if b.len() != 10
        || b[4] != b'-'
        || b[7] != b'-'
        || b.iter()
            .enumerate()
            .any(|(i, b)| i != 4 && i != 7 && !b.is_ascii_digit())
    {
        return false;
    }
    let year: u16 = value[..4].parse().unwrap_or(0);
    let month: usize = value[5..7].parse().unwrap_or(0);
    let day: u8 = value[8..].parse().unwrap_or(0);
    if !(1970..=9999).contains(&year) || !(1..=12).contains(&month) {
        return false;
    }
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let max = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ][month - 1];
    (1..=max).contains(&day)
}
fn calendar(time_of_day: &str, zone: Option<&str>) -> Result<(), Failure> {
    if !time(time_of_day) || zone.is_some_and(|z| validate_zone(z).is_err()) {
        return Err(Failure::BadArguments);
    }
    Ok(())
}
fn parse(name: &str, value: &Value) -> Result<Call, Failure> {
    let call = match name {
        "set_timer" => {
            let a: Timer = decode(value)?;
            seconds(a.seconds)?;
            Call::Set {
                text: a.text,
                when: When::After { seconds: a.seconds },
            }
        }
        "set_reminder" => {
            let a: Reminder = decode(value)?;
            calendar(&a.time, a.zone.as_deref())?;
            if a.day.as_deref().is_some_and(|d| !day(d)) {
                return Err(Failure::BadArguments);
            }
            Call::Set {
                text: a.text,
                when: When::At {
                    time: a.time,
                    day: a.day,
                    zone: a.zone,
                    fold: a.fold,
                },
            }
        }
        "repeat_reminder" => {
            let a: Repeating = decode(value)?;
            calendar(&a.time, a.zone.as_deref())?;
            let when = match (a.every, a.weekday) {
                (Every::Daily, None) => When::Daily {
                    time: a.time,
                    zone: a.zone,
                },
                (Every::Weekly, Some(weekday)) => When::Weekly {
                    weekday,
                    time: a.time,
                    zone: a.zone,
                },
                _ => return Err(Failure::BadArguments),
            };
            Call::Set { text: a.text, when }
        }
        "list_reminders" => {
            let a: Listing = decode(value)?;
            Call::List {
                after_id: a.after_id,
                pending_only: a.pending_only,
            }
        }
        "cancel_reminder" => {
            let a: Cancel = decode(value)?;
            if a.id == 0 {
                return Err(Failure::BadArguments);
            }
            Call::Cancel { id: a.id }
        }
        "snooze_reminder" => {
            let a: Snooze = decode(value)?;
            seconds(a.seconds)?;
            if a.id == 0 {
                return Err(Failure::BadArguments);
            }
            Call::Snooze {
                id: a.id,
                seconds: a.seconds,
            }
        }
        _ => return Err(Failure::BadArguments),
    };
    if matches!(&call, Call::Set { text, .. } if !valid_text(text)) {
        return Err(Failure::BadArguments);
    }
    Ok(call)
}

fn identity(reply: &Value) -> Result<(&str, u64), Failure> {
    let store = reply["store_id"]
        .as_str()
        .filter(|s| {
            s.len() == 32
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
        .ok_or(Failure::Unavailable)?;
    let revision = reply["revision"].as_u64().ok_or(Failure::Unavailable)?;
    Ok((store, revision))
}
/// The due time's local wall clock, `HH:MM`, from `local` (RFC 9557,
/// "2026-10-25T21:07:34.939+07:00[Asia/Ho_Chi_Minh]"): the time the model
/// says. On lamp-4ace on 25 Oct it read "07:34" out of such a string and
/// said "seven thirty-four" for a reminder due at 21:07, and "eight
/// forty-seven" for a timer due at 21:08.
pub fn clock_of(local: &str) -> Option<String> {
    let hm = local.split_once('T')?.1.get(..5)?;
    let b = hm.as_bytes();
    (b[2] == b':' && b[..2].iter().all(u8::is_ascii_digit) && b[3..].iter().all(u8::is_ascii_digit))
        .then(|| hm.to_owned())
}
fn resolved(reply: &Value) -> Result<(i64, &str), Failure> {
    let due = reply["due_unix_ms"].as_i64().ok_or(Failure::Uncertain)?;
    let local = reply["local"].as_str().ok_or(Failure::Uncertain)?;
    Deadline {
        unix_ms: due,
        elapsed: None,
        repeat: None,
        local: local.into(),
    }
    .check()
    .map_err(|_| Failure::Uncertain)?;
    Ok((due, local))
}
fn committed(request: &Request, reply: &Value) -> Result<Value, Failure> {
    let guard = request.guard().ok_or(Failure::Uncertain)?;
    let (store, revision) = identity(reply).map_err(|_| Failure::Uncertain)?;
    if store != guard.store_id || guard.revision.checked_add(1) != Some(revision) {
        return Err(Failure::Uncertain);
    }
    let id = reply["id"]
        .as_u64()
        .filter(|n| *n > 0)
        .ok_or(Failure::Uncertain)?;
    let delivery = reply["delivery"]
        .as_u64()
        .filter(|n| *n > 0)
        .ok_or(Failure::Uncertain)?;
    match request {
        Request::Set { .. } if delivery == 1 => {
            let (due, local) = resolved(reply)?;
            Ok(
                json!({"status":"committed","id":id,"delivery":delivery,"due_unix_ms":due,"local":local,"clock":clock_of(local),"message":"The timer or reminder is saved. Say its time as `clock`, the local HH:MM it is due, never read from `local`."}),
            )
        }
        Request::Cancel { id: asked, .. } if *asked == id && reply["cancelled"] == true => Ok(
            json!({"status":"committed","id":id,"cancelled":true,"message":"The selected timer or whole reminder series is cancelled."}),
        ),
        Request::Snooze {
            id: asked,
            delivery: old,
            ..
        } if *asked == id && old.checked_add(1) == Some(delivery) && reply["snoozed"] == true => {
            let (due, local) = resolved(reply)?;
            Ok(
                json!({"status":"committed","id":id,"delivery":delivery,"snoozed":true,"due_unix_ms":due,"local":local,"clock":clock_of(local),"message":"The current occurrence is postponed; its repeating schedule is unchanged. Say its new time as `clock`, the local HH:MM, never read from `local`."}),
            )
        }
        _ => Err(Failure::Uncertain),
    }
}

#[derive(Deserialize)]
struct Item {
    id: u64,
    delivery: u64,
    text: String,
    kind: String,
    state: String,
    due_unix_ms: i64,
    local: String,
    repeat: Option<Repeat>,
    snoozed: bool,
    late: bool,
}
fn item(value: &Value) -> Result<Item, Failure> {
    if value.get("repeat").is_none() {
        return Err(Failure::Unavailable);
    }
    let item: Item = serde_json::from_value(value.clone()).map_err(|_| Failure::Unavailable)?;
    if item.id == 0
        || item.delivery == 0
        || !valid_text(&item.text)
        || !matches!(item.kind.as_str(), "timer" | "reminder")
        || !matches!(
            item.state.as_str(),
            "pending" | "scheduled" | "waiting_for_clock"
        )
    {
        return Err(Failure::Unavailable);
    }
    resolved(value).map_err(|_| Failure::Unavailable)?;
    if let Some(repeat) = &item.repeat {
        let (time_of_day, zone, date) = match repeat {
            Repeat::Daily { time, zone, date }
            | Repeat::Weekly {
                time, zone, date, ..
            } => (time, zone, date),
        };
        if !time(time_of_day) || validate_zone(zone).is_err() || date.len() != 10 || !day(date) {
            return Err(Failure::Unavailable);
        }
    }
    Ok(item)
}
fn page(reply: &Value, after_id: u64, pending_only: bool) -> Result<Value, Failure> {
    let values = reply["items"]
        .as_array()
        .filter(|v| v.len() <= 8)
        .ok_or(Failure::Unavailable)?;
    let server_next = match reply.get("next_after_id") {
        Some(Value::Null) => None,
        Some(v) => Some(v.as_u64().ok_or(Failure::Unavailable)?),
        None => return Err(Failure::Unavailable),
    };
    let mut checked = Vec::new();
    let mut previous = after_id;
    for value in values {
        let i = item(value)?;
        if i.id <= previous || (pending_only && i.state != "pending") {
            return Err(Failure::Unavailable);
        }
        previous = i.id;
        checked.push(json!({"id":i.id,"kind":i.kind,"text":i.text,"state":i.state,"local":i.local,"clock":clock_of(&i.local),"due_unix_ms":i.due_unix_ms,"repeat":i.repeat,"snoozed":i.snoozed,"late":i.late}));
    }
    if server_next.is_some_and(|n| values.is_empty() || n != previous) {
        return Err(Failure::Unavailable);
    }
    let mut items = Vec::new();
    for value in &checked {
        items.push(value.clone());
        let mut probe = json!({"items":items,"next_after_id":u64::MAX,"pending_only":pending_only,"complete":false});
        if pending_only {
            // Room kept for the count of what is not yet due (#1595).
            probe["not_yet_due"] = json!(u64::MAX);
            probe["next"] = json!(NOT_YET_DUE);
        }
        if probe.to_string().len() > MAX_RESULT_BYTES {
            items.pop();
            break;
        }
    }
    if items.is_empty() && !checked.is_empty() {
        return Err(Failure::Unavailable);
    }
    let next = if items.len() < checked.len() {
        items.last().and_then(|i| i["id"].as_u64())
    } else {
        server_next
    };
    Ok(
        json!({"items":items,"next_after_id":next,"pending_only":pending_only,"complete":next.is_none()}),
    )
}
