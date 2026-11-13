//! Calendar events a read found, kept for the next request
//! (`LAMP_SERVICE_READ_IDS_KEPT`, tracker row C132, the owner's option 2 of
//! 12 Nov).
//!
//! Moving or cancelling an event made in an earlier conversation could never
//! complete: the model had no `event_id`, so it called `service_read` first;
//! the after-read rule ([`crate::services::after_read`], `AFTER_READ`) then
//! refused the write in that same request, and the next request read again,
//! since a read's words are kept nowhere, and was refused again (lamp-06c1,
//! 12 Nov, 09:4x: "ask me again" in a loop). The after-read rule stays
//! exactly as it is: no write is ever prepared in a read's own request.
//!
//! With the switch on, a Google Calendar `events` read keeps, for the
//! person whose request made it, only each event's id, calendar, title and
//! start and end: never a description, a place, a guest or any other text
//! of the read, and at most [`KEPT_MOST`] events. The person's next spoken
//! requests are told them ([`note_for`]), so "yes, cancel it" can name the
//! event by its id in a `service_write` without reading again; the write
//! then goes through the gate, the services' own read of the event, its
//! read-back and a spoken yes, as every edit does.
//!
//! - **Whose.** Kept under the services' owner of the line that read
//!   (`services::owner_of`); nobody known, nothing is kept. Another
//!   person's request is told none, and a write of theirs naming one is
//!   refused ([`NOT_THEIRS`]).
//! - **How long.** [`KEPT_FOR`] after the read, or until the conversation
//!   is closed (`thread::Thread::close_conversation`), whichever is first;
//!   then only the ids stay, titles and times dropped, so a write naming
//!   one is told to read again ([`EXPIRED`]). A forget or a privacy change
//!   (`services::forget_waiting`) drops everything.
//! - **What a write may name.** An id no read kept (a made event's, from
//!   its record) goes on as before: Google's own read decides.
use serde_json::{Value, json};
use std::time::{Duration, Instant};

/// Only the exact value `on`, on the second route.
pub const ENVIRONMENT: &str = "LAMP_SERVICE_READ_IDS_KEPT";

/// How long a read's events are kept: five minutes. The yes to "Shall I
/// cancel it?" comes in seconds, and a side question or a second look
/// takes a minute or two; longer, and the person may well have moved the
/// event elsewhere, or be in another talk, and the lamp should look again.
/// Google's own read before the read-back still decides whether it is there.
pub const KEPT_FOR: Duration = Duration::from_secs(5 * 60);
/// The most events kept of one read: its first ones, as the read lists
/// them. "Cancel my dentist on Friday" finds one or two; five covers a
/// day's list without telling a week of them every turn.
pub const KEPT_MOST: usize = 5;
/// The most people a read is kept for at once (the owners of the lamp).
const PEOPLE_MOST: usize = 4;
/// The most bytes of the note told ([`note_for`]): its rule and the data
/// note take about 420, a shared calendar's id about 100 more, and five
/// events with 48-byte titles and Google's recurring-instance ids about 160
/// each; under three quarters of the records note's
/// (`service_records::told::NOTE_BYTES`, 1,900). An event whose line does
/// not fit is not told.
pub const NOTE_BYTES: usize = 1_400;
/// The most bytes of a title in the note.
const TITLE_BYTES: usize = 48;
/// The most bytes of a start or an end ("2026-11-14T10:00:00+07:00" is 25).
const TIME_BYTES: usize = 32;
/// The most bytes of an id kept: Google's own bound (`writes::EVENT_ID_MOST`).
const ID_MOST: usize = 1_024;

/// The rule told with the events.
pub const RULE: &str = "To move or cancel one they asked for and confirmed, call service_write now with action update_event or delete_event and its event_id; don't read again. It is read back before anything changes.";

/// What a write is told when it names an event a read kept but no longer
/// keeps: too long ago, or the conversation was closed.
pub const EXPIRED: &str = "Not done: that event_id came from a calendar read that is no longer kept (a few minutes, or the conversation ended). Call service_read to look for the event again, say what you found, and ask them again.";

/// What a write is told when it names an event kept from another person's
/// read.
pub const NOT_THEIRS: &str = "Not done: that event_id came from someone else's calendar read. Call service_read to look for the event, say what you found, and ask them again.";

/// One event a read found, as kept.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub id: String,
    pub calendar: String,
    pub title: String,
    pub start: String,
    pub end: String,
}

/// One person's events of their last calendar read.
#[derive(Clone, Debug)]
struct Kept {
    person: String,
    at: Instant,
    events: Vec<Event>,
    /// Past [`KEPT_FOR`] or the conversation closed: only the ids stay.
    expired: bool,
}

impl Kept {
    fn expire(&mut self) {
        self.expired = true;
        for event in &mut self.events {
            event.title.clear();
            event.start.clear();
            event.end.clear();
        }
    }
}

#[cfg(not(test))]
mod kept {
    use super::Kept;
    use std::sync::{LazyLock, Mutex};

    static KEPT: Mutex<Vec<Kept>> = Mutex::new(Vec::new());

    pub fn with<R>(f: impl FnOnce(&mut Vec<Kept>) -> R) -> R {
        f(&mut KEPT.lock().unwrap_or_else(|poisoned| poisoned.into_inner()))
    }

    pub fn on() -> bool {
        static ON: LazyLock<bool> =
            LazyLock::new(|| std::env::var(super::ENVIRONMENT).as_deref() == Ok("on"));
        *ON
    }
}

/// Tests: each test thread its own.
#[cfg(test)]
mod kept {
    use super::Kept;
    use std::cell::{Cell, RefCell};

    thread_local! {
        static KEPT: RefCell<Vec<Kept>> = const { RefCell::new(Vec::new()) };
        pub static ON: Cell<bool> = const { Cell::new(false) };
    }

    pub fn with<R>(f: impl FnOnce(&mut Vec<Kept>) -> R) -> R {
        KEPT.with(|cell| f(&mut cell.borrow_mut()))
    }

    pub fn on() -> bool {
        ON.with(Cell::get)
    }
}

/// Whether a read's events are kept ([`ENVIRONMENT`]).
pub fn on() -> bool {
    kept::on()
}

/// Tests: on or off on this thread.
#[cfg(test)]
pub fn on_for_test(on: bool) {
    kept::ON.with(|cell| cell.set(on));
}

/// Whether `service` names Google Calendar, as the model may say it.
fn is_calendar(service: &str) -> bool {
    service.trim().to_ascii_lowercase().replace([' ', '-'], "_") == "google_calendar"
}

/// One line of the service's text, at most `most` bytes, cut at a
/// character.
fn tidy(text: &str, most: usize) -> String {
    let mut out = String::new();
    for c in text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
    {
        if out.len() + c.len_utf8() > most {
            break;
        }
        out.push(c);
    }
    out
}

/// An event's start or end as Google gives it: `dateTime`, else `date`.
fn time_of(item: &Value, key: &str) -> String {
    let at = item.get(key);
    let text = at
        .and_then(|at| at.get("dateTime").or_else(|| at.get("date")))
        .or(at)
        .and_then(Value::as_str)
        .unwrap_or_default();
    tidy(text, TIME_BYTES)
}

/// What a calendar read's answer `told` found, kept: of its first
/// [`KEPT_MOST`] events with an id, only the id, `calendar`, the title (none
/// with a code's shape) and the start and end.
pub fn events_of(told: &Value, calendar: &str) -> Vec<Event> {
    told.get("quoted")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let id = item.get("id").and_then(Value::as_str)?.trim();
            let fits = !id.is_empty()
                && id.len() <= ID_MOST
                && !id.contains(['/', '\\', '?', '#'])
                && !id.chars().any(char::is_whitespace);
            fits.then(|| Event {
                id: id.to_owned(),
                calendar: calendar.to_owned(),
                // A title with a code's shape is never kept, as a read's
                // record keeps none (`service_records::found`).
                title: Some(tidy(
                    item.get("summary")
                        .or_else(|| item.get("title"))
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                    TITLE_BYTES,
                ))
                .filter(|title| !lamp_services::mail::has_code_shape(title))
                .unwrap_or_default(),
                start: time_of(item, "start"),
                end: time_of(item, "end"),
            })
        })
        .take(KEPT_MOST)
        .collect()
}

/// A `service_read` with `args` answered `told` for `owner`
/// (`services::run`, before any brief): a Google Calendar `events` read's
/// events are kept for them, in place of any they had. Nobody known, an
/// error or another read: nothing changes.
pub fn read_answered(owner: Option<&str>, args: &Value, told: &Value, now: Instant) {
    if !on() {
        return;
    }
    let Some(owner) = owner.filter(|owner| !owner.is_empty()) else {
        return;
    };
    let text = |key: &str| args.get(key).and_then(Value::as_str).unwrap_or_default();
    if !is_calendar(text("service")) || text("what") != "events" || told.get("error").is_some() {
        return;
    }
    let calendar = match text("calendar").trim() {
        "" => "primary",
        given => given,
    };
    let events = events_of(told, calendar);
    let count = events.len();
    kept::with(|kept| {
        kept.retain(|held| held.person != owner);
        if events.is_empty() {
            return;
        }
        if kept.len() >= PEOPLE_MOST {
            kept.remove(0);
        }
        kept.push(Kept {
            person: owner.to_owned(),
            at: now,
            events,
            expired: false,
        });
    });
    eprintln!(
        "lamp-voice: a calendar read's {count} event id(s) kept for its asker ({ENVIRONMENT})"
    );
}

/// Each kept read past [`KEPT_FOR`] at `now` expired.
fn refresh(kept: &mut [Kept], now: Instant) {
    for held in kept.iter_mut() {
        if !held.expired && now.saturating_duration_since(held.at) > KEPT_FOR {
            held.expire();
        }
    }
}

/// The conversation was closed (`thread::Thread::close_conversation`):
/// every kept read expires, its ids left only to say so.
pub fn conversation_ended() {
    kept::with(|kept| kept.iter_mut().for_each(Kept::expire));
}

/// A forget or a privacy change (`services::forget_waiting`): nothing is
/// kept.
pub fn forget() {
    kept::with(Vec::clear);
}

/// The note for a line of `owner`'s (`services::owner_of`) at `now`: their
/// own kept events, one line each, with [`RULE`] and the services'
/// `DATA_NOTE`, at most [`NOTE_BYTES`]. None for anyone else, with none
/// kept, or past [`KEPT_FOR`].
pub fn note_for(owner: Option<&str>, now: Instant) -> Option<String> {
    if !on() {
        return None;
    }
    let owner = owner.filter(|owner| !owner.is_empty())?;
    let events = kept::with(|kept| {
        refresh(kept, now);
        kept.iter()
            .find(|held| held.person == owner && !held.expired)
            .map(|held| held.events.clone())
    })?;
    let calendar = events
        .first()
        .map(|event| event.calendar.as_str())
        .filter(|calendar| *calendar != "primary")
        .map(|calendar| format!(" Give calendar {calendar} with it."))
        .unwrap_or_default();
    let mut note = format!(
        "(For you to know, not to answer unless asked: Google Calendar events their last calendar read found, kept a few minutes. {RULE}{calendar} Titles in «» are what the service gave: {}",
        crate::services::DATA_NOTE
    );
    let mut told = 0;
    for event in &events {
        let when = crate::service_records::told::event_time(
            &json!({"start": event.start, "end": event.end}),
        )
        .map(|when| format!(" · {when}"))
        .unwrap_or_default();
        let line = format!("\n- «{}»{when} · event_id {}", event.title, event.id);
        if note.len() + line.len() + 1 > NOTE_BYTES {
            continue;
        }
        note.push_str(&line);
        told += 1;
    }
    if told == 0 {
        return None;
    }
    note.push(')');
    Some(note)
}

/// A write of `owner`'s with `args` before it is prepared (`services::gate`):
/// a calendar edit naming an event their own read keeps goes on, its
/// calendar filled in when the call gave none; one whose read expired is
/// [`EXPIRED`], one only another person's read keeps is [`NOT_THEIRS`].
/// Any other write, or an id no read keeps, goes on unchanged.
pub fn check_write(owner: Option<&str>, args: &mut Value, now: Instant) -> Result<(), Value> {
    if !on() {
        return Ok(());
    }
    let text = |key: &str| args.get(key).and_then(Value::as_str).map(str::to_owned);
    let edit = matches!(
        text("action").as_deref(),
        Some("update_event" | "delete_event")
    );
    let (true, true, Some(id)) = (
        edit,
        is_calendar(&text("service").unwrap_or_default()),
        text("event_id").map(|id| id.trim().to_owned()),
    ) else {
        return Ok(());
    };
    let owner = owner.unwrap_or_default();
    let (mine, others) = kept::with(|kept| {
        refresh(kept, now);
        let find = |theirs: bool| {
            kept.iter()
                .filter(|held| (held.person == owner) == theirs)
                .find_map(|held| {
                    held.events
                        .iter()
                        .find(|event| event.id == id)
                        .map(|event| (held.expired, event.calendar.clone()))
                })
        };
        (find(true), find(false).is_some())
    });
    match mine {
        Some((false, calendar)) => {
            if text("calendar").is_none_or(|given| given.trim().is_empty())
                && let Some(args) = args.as_object_mut()
            {
                args.insert("calendar".into(), json!(calendar));
            }
            Ok(())
        }
        Some((true, _)) => Err(json!({ "error": EXPIRED })),
        None if others => Err(json!({ "error": NOT_THEIRS })),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests;
