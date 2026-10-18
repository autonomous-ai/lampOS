//! Durable scheduling policy. Every mutation is prepared on a copy, committed,
//! then made visible. No publication or speech is a storage acknowledgement.
use crate::{
    protocol::{Guard, MAX_PAGE_SIZE, MAX_REPLY_BYTES, Request},
    store::Store,
    time::{self, ClockSample, Deadline, When},
};
use lamp_msgs::{ClockStamp, ScheduleDue, ScheduleNotice};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fmt, io};

pub const MAX_ITEMS: usize = 128;
pub const MAX_TEXT_BYTES: usize = 300;
pub const LATE_AFTER_MS: u64 = 1000;

pub trait Persistence {
    fn load(&self) -> io::Result<Option<Vec<u8>>>;
    fn commit(&mut self, bytes: &[u8]) -> io::Result<()>;
    /// Durably retain the current store under another name, without replacing
    /// anything, and start an empty one. Backends without that operation refuse
    /// newer schemas; they must never overwrite them with an empty snapshot.
    fn set_aside(&mut self, _label: &str) -> io::Result<String> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "this persistence cannot set a newer schedule aside",
        ))
    }
}

impl Persistence for Store {
    fn load(&self) -> io::Result<Option<Vec<u8>>> {
        self.load()
    }
    fn commit(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.commit(bytes)
    }
    fn set_aside(&mut self, label: &str) -> io::Result<String> {
        self.set_aside(label)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub code: &'static str,
    pub message: &'static str,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for Error {}

fn err(code: &'static str, message: &'static str) -> Error {
    Error { code, message }
}
fn storage(_: impl fmt::Display) -> Error {
    err(
        "storage",
        "Schedule storage is unavailable; restart and inspect it before retrying.",
    )
}
fn bad_time(_: impl fmt::Display) -> Error {
    err(
        "bad_time",
        "The time, zone, clock trust or daylight-saving choice is not valid.",
    )
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Pending {
    overdue_at_detection_ms: u64,
    recovered_late: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Item {
    id: u64,
    text: String,
    created_unix_ms: i64,
    request_id: String,
    deadline: Deadline,
    snooze: Option<Deadline>,
    delivery: u64,
    pending: Option<Pending>,
}

impl Item {
    fn effective(&self) -> &Deadline {
        self.snooze.as_ref().unwrap_or(&self.deadline)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Receipt {
    request: Request,
    response: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct State {
    schema: u32,
    store_id: String,
    revision: u64,
    next_id: u64,
    items: Vec<Item>,
    last_receipt: Option<Receipt>,
}

fn valid_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn valid_request_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-.:".contains(&c))
}

fn valid_text(text: &str) -> bool {
    !text.trim().is_empty() && text.len() <= MAX_TEXT_BYTES && !text.chars().any(char::is_control)
}

impl State {
    fn check(&self) -> Result<(), Error> {
        let invalid = || {
            err(
                "storage",
                "Schedule state has an unsupported or invalid shape.",
            )
        };
        if self.schema != 1
            || !valid_id(&self.store_id)
            || self.next_id == 0
            || self.items.len() > MAX_ITEMS
        {
            return Err(invalid());
        }
        let mut last_id = 0;
        for item in &self.items {
            if item.id <= last_id
                || item.id >= self.next_id
                || item.delivery == 0
                || !valid_text(&item.text)
                || !valid_request_id(&item.request_id)
                || !(time::MIN_UNIX_MS..=time::MAX_UNIX_MS).contains(&item.created_unix_ms)
            {
                return Err(invalid());
            }
            item.deadline.check().map_err(|_| invalid())?;
            if let Some(snooze) = &item.snooze {
                snooze.check().map_err(|_| invalid())?;
                if snooze.repeat.is_some() || snooze.elapsed.is_none() {
                    return Err(invalid());
                }
            }
            last_id = item.id;
        }
        if let Some(receipt) = &self.last_receipt {
            let guard = receipt.request.guard().ok_or_else(invalid)?;
            if guard.store_id != self.store_id
                || !valid_request_id(&guard.request_id)
                || guard.revision >= self.revision
                || receipt.response["ok"] != true
                || receipt.response["store_id"] != self.store_id
                || receipt.response["revision"]
                    .as_u64()
                    .is_none_or(|r| r > self.revision || r <= guard.revision)
                || serde_json::to_vec(receipt).map_err(|_| invalid())?.len() > 16_384
            {
                return Err(invalid());
            }
            let receipt_revision = guard.revision.checked_add(1).ok_or_else(invalid)?;
            let body = match &receipt.request {
                Request::Set { text, .. } => {
                    let item = self.items.last().ok_or_else(invalid)?;
                    if item.id != self.next_id - 1
                        || item.request_id != guard.request_id
                        || item.text != text.trim()
                        || item.delivery != 1
                        || item.snooze.is_some()
                        || !valid_text(text)
                    {
                        return Err(invalid());
                    }
                    json!({"id":item.id,"delivery":1,"due_unix_ms":item.deadline.unix_ms,"local":item.deadline.local})
                }
                Request::Cancel { id, .. } => {
                    if *id == 0 || self.items.iter().any(|i| i.id == *id) {
                        return Err(invalid());
                    }
                    let delivery = receipt.response["delivery"]
                        .as_u64()
                        .filter(|d| *d > 0)
                        .ok_or_else(invalid)?;
                    json!({"cancelled":true,"id":id,"delivery":delivery})
                }
                Request::Snooze { id, delivery, .. } => {
                    let item = self
                        .items
                        .iter()
                        .find(|i| i.id == *id)
                        .ok_or_else(invalid)?;
                    let until = item.snooze.as_ref().ok_or_else(invalid)?;
                    if *delivery == 0 || delivery.checked_add(1) != Some(item.delivery) {
                        return Err(invalid());
                    }
                    json!({"snoozed":true,"id":id,"delivery":item.delivery,"due_unix_ms":until.unix_ms,"local":until.local})
                }
                Request::Ack { id, delivery, .. } => {
                    if *id == 0
                        || *delivery == 0
                        || self.items.iter().find(|i| i.id == *id).is_some_and(|i| {
                            delivery.checked_add(1) != Some(i.delivery) || i.snooze.is_some()
                        })
                    {
                        return Err(invalid());
                    }
                    json!({"acknowledged":true,"id":id,"delivery":delivery})
                }
                _ => return Err(invalid()),
            };
            if receipt.response != with_identity(body, &self.store_id, receipt_revision) {
                return Err(invalid());
            }
        }
        Ok(())
    }
}

pub struct Engine<P: Persistence> {
    persistence: P,
    state: State,
    zone: String,
    last_clock: ClockSample,
    recovered: BTreeSet<u64>,
    failed: bool,
}

impl<P: Persistence> Engine<P> {
    /// `new_store_id` is fresh random data in serve, and injectable in tests.
    /// Existing stores retain their identity and monotonically rising IDs.
    pub fn open(
        mut persistence: P,
        zone: &str,
        now: &ClockSample,
        new_store_id: &str,
    ) -> Result<Self, Error> {
        time::validate_zone(zone).map_err(bad_time)?;
        now.check().map_err(bad_time)?;
        let bytes = persistence.load().map_err(storage)?;
        let existing = bytes.is_some();
        let state = match bytes {
            Some(bytes) => serde_json::from_slice::<State>(&bytes).map_err(storage)?,
            None => State {
                schema: 1,
                store_id: new_store_id.into(),
                revision: 0,
                next_id: 1,
                items: Vec::new(),
                last_receipt: None,
            },
        };
        state.check()?;
        if !existing {
            persistence
                .commit(&serde_json::to_vec(&state).map_err(storage)?)
                .map_err(storage)?;
        }
        let recovered = state.items.iter().map(|item| item.id).collect();
        Ok(Self {
            persistence,
            state,
            zone: zone.into(),
            last_clock: now.clone(),
            recovered,
            failed: false,
        })
    }

    pub fn failed(&self) -> bool {
        self.failed
    }
    pub fn revision(&self) -> u64 {
        self.state.revision
    }
    pub fn store_id(&self) -> &str {
        &self.state.store_id
    }

    fn observe(&mut self, now: &ClockSample) -> Result<(), Error> {
        if self.failed {
            return Err(storage("previous failure"));
        }
        now.check().map_err(bad_time)?;
        if now.boot_id != self.last_clock.boot_id || now.boot_ms < self.last_clock.boot_ms {
            return Err(err(
                "clock",
                "The boot clock changed or moved backwards; restart Schedule.",
            ));
        }
        self.last_clock = now.clone();
        Ok(())
    }

    fn save(&mut self, candidate: State) -> Result<(), Error> {
        candidate.check()?;
        let bytes = serde_json::to_vec(&candidate).map_err(storage)?;
        if let Err(error) = self.persistence.commit(&bytes) {
            // A failed sync may follow a committed transaction. Publishing the
            // older in-memory state or retrying it would hide that ambiguity.
            self.failed = true;
            return Err(storage(error));
        }
        self.state = candidate;
        Ok(())
    }

    /// One bounded scan. Pending occurrences never become future again after a
    /// clock correction. A repeat advances only when its exact delivery is acked,
    /// then to its next occurrence still ahead: after time off it is said once.
    pub fn tick(&mut self, now: &ClockSample) -> Result<bool, Error> {
        self.observe(now)?;
        let mut next = self.state.clone();
        let mut changed = false;
        for item in &mut next.items {
            if item.pending.is_some() {
                continue;
            }
            if let Some(remaining) = time::remaining_ms(item.effective(), now).map_err(bad_time)? {
                let recovered = self.recovered.remove(&item.id);
                if remaining <= 0 {
                    item.pending = Some(Pending {
                        overdue_at_detection_ms: remaining.unsigned_abs(),
                        recovered_late: recovered && remaining < 0,
                    });
                    changed = true;
                }
            }
        }
        if changed {
            next.revision = next
                .revision
                .checked_add(1)
                .ok_or_else(|| err("full", "Schedule revision is exhausted."))?;
            self.save(next)?;
        }
        Ok(changed)
    }

    /// The occurrence a pending repeat stands for: after time off its newest
    /// missed one, so it is said once and late by that one's lateness, not by
    /// the oldest's (#811). The ack steps over the rest (`time::next_ahead`).
    /// The stored deadline stays as it is: its last receipt names it.
    fn shown(item: &Item, now: &ClockSample) -> Deadline {
        if item.snooze.is_some() || item.pending.is_none() {
            return item.effective().clone();
        }
        time::newest_due(&item.deadline, now).unwrap_or_else(|_| item.deadline.clone())
    }

    fn late(&self, item: &Item, now: &ClockSample) -> (bool, Option<u64>) {
        let Some(pending) = &item.pending else {
            return (false, None);
        };
        let shown = Self::shown(item, now);
        let stepped = shown != *item.effective();
        let late_by = time::remaining_ms(&shown, now)
            .ok()
            .flatten()
            .map(|remaining| {
                if remaining < 0 {
                    remaining.unsigned_abs()
                } else {
                    0
                }
            })
            .map(|ms| {
                if stepped {
                    ms
                } else {
                    ms.max(pending.overdue_at_detection_ms)
                }
            });
        (
            pending.recovered_late
                || late_by.unwrap_or(pending.overdue_at_detection_ms) > LATE_AFTER_MS,
            late_by,
        )
    }

    fn item_view(&self, item: &Item, now: &ClockSample) -> Value {
        let remaining = time::remaining_ms(item.effective(), now).ok().flatten();
        let (late, late_by_ms) = self.late(item, now);
        json!({
            "id": item.id, "delivery": item.delivery, "text": item.text,
            "request_id": item.request_id,
            "state": if item.pending.is_some() { "pending" } else if remaining.is_none() { "waiting_for_clock" } else { "scheduled" },
            "due_unix_ms": Self::shown(item, now).unix_ms, "local": Self::shown(item, now).local,
            "original_due_unix_ms": item.deadline.unix_ms,
            "repeat": item.deadline.repeat, "snoozed": item.snooze.is_some(),
            "remaining_ms": remaining.map(|r| r.max(0)), "late": late, "late_by_ms": late_by_ms
        })
    }

    fn reply(&self, body: Value) -> Value {
        with_identity(body, &self.state.store_id, self.state.revision)
    }

    pub fn counts(&self, now: &ClockSample) -> (usize, usize, usize) {
        let pending = self
            .state
            .items
            .iter()
            .filter(|i| i.pending.is_some())
            .count();
        let waiting = self
            .state
            .items
            .iter()
            .filter(|i| {
                i.pending.is_none()
                    && time::remaining_ms(i.effective(), now)
                        .ok()
                        .flatten()
                        .is_none()
            })
            .count();
        (self.state.items.len(), pending, waiting)
    }

    pub fn due(&self, now: &ClockSample, stamp: ClockStamp, ready: bool) -> ScheduleDue {
        let (_, pending, waiting) = self.counts(now);
        let ready = ready && !self.failed;
        let oldest = ready
            .then(|| {
                self.state
                    .items
                    .iter()
                    .filter(|i| i.pending.is_some())
                    // By the occurrence each stands for: after time off, a
                    // repeat's newest missed one, not its stored deadline.
                    .min_by_key(|i| (Self::shown(i, now).unix_ms, i.id))
            })
            .flatten()
            .map(|item| {
                let (late, late_by_ms) = self.late(item, now);
                ScheduleNotice {
                    id: item.id,
                    delivery: item.delivery,
                    due_unix_ms: Self::shown(item, now).unix_ms,
                    late_by_ms,
                    late,
                }
            });
        ScheduleDue {
            stamp,
            store_id: self.state.store_id.clone(),
            revision: self.state.revision,
            enabled: true,
            ready,
            pending: pending as u32,
            waiting_for_clock: waiting as u32,
            oldest,
        }
    }

    pub fn answer(&mut self, request: Request, now: &ClockSample) -> Result<Value, Error> {
        self.tick(now)?;
        match &request {
            Request::Status {} => {
                let (items, pending, waiting) = self.counts(now);
                return Ok(self.reply(json!({"items":items,"pending":pending,"waiting_for_clock":waiting,
                    "wall_trusted":now.wall_trusted,"zone":self.zone,"max_items":MAX_ITEMS,
                    "last_receipt":self.state.last_receipt.as_ref().map(|r| json!({
                        "request_id":r.request.guard().expect("validated mutation receipt").request_id,
                        "op":r.request.name(),"response":r.response}))})));
            }
            Request::List {
                after_id,
                limit,
                pending_only,
            } => {
                if !(1..=MAX_PAGE_SIZE).contains(limit) {
                    return Err(err("bad_request", "Page limit must be from 1 to 8."));
                }
                let available: Vec<_> = self
                    .state
                    .items
                    .iter()
                    .filter(|item| item.id > *after_id && (!pending_only || item.pending.is_some()))
                    .take(usize::from(*limit) + 1)
                    .collect();
                let mut views = Vec::new();
                for item in available.iter().take(usize::from(*limit)) {
                    views.push(self.item_view(item, now));
                    // Reserve the largest cursor representation. Text is never
                    // truncated just to fit a page, including escaped Unicode.
                    let probe = self.reply(json!({"items":views,"next_after_id":u64::MAX}));
                    if serde_json::to_vec(&probe).map_err(storage)?.len() > MAX_REPLY_BYTES {
                        views.pop();
                        break;
                    }
                }
                if views.is_empty() && !available.is_empty() {
                    return Err(err(
                        "too_long",
                        "A single item cannot fit in a bounded response.",
                    ));
                }
                let next_after_id =
                    (views.len() < available.len()).then(|| available[views.len() - 1].id);
                return Ok(self.reply(json!({"items":views,"next_after_id":next_after_id})));
            }
            Request::Get { id } => {
                let item = self
                    .state
                    .items
                    .iter()
                    .find(|i| i.id == *id)
                    .ok_or_else(|| {
                        err("not_found", "That reminder or timer is no longer active.")
                    })?;
                return Ok(self.reply(json!({"item":self.item_view(item,now)})));
            }
            _ => {}
        }
        let guard = request.guard().expect("mutation has a guard");
        if !valid_request_id(&guard.request_id) || guard.store_id != self.state.store_id {
            return Err(err(
                "bad_guard",
                "Use the current store identity and a valid request ID.",
            ));
        }
        if let Some(receipt) = &self.state.last_receipt
            && receipt.request == request
        {
            return Ok(receipt.response.clone());
        }
        self.guard(guard)?;
        let mut next = self.state.clone();
        let body = match &request {
            Request::Set { text, when, .. } => {
                if !valid_text(text) {
                    return Err(err(
                        "bad_text",
                        "Reminder text must contain 1 to 300 UTF-8 bytes and no control characters.",
                    ));
                }
                if next.items.len() == MAX_ITEMS {
                    return Err(err(
                        "full",
                        "There are already 128 active reminders and timers; cancel or acknowledge one.",
                    ));
                }
                let deadline = time::resolve(when, now, &self.zone).map_err(bad_time)?;
                let id = next.next_id;
                next.next_id = id
                    .checked_add(1)
                    .ok_or_else(|| err("full", "Schedule identifiers are exhausted."))?;
                next.items.push(Item {
                    id,
                    text: text.trim().into(),
                    created_unix_ms: now.unix_ms,
                    request_id: guard.request_id.clone(),
                    deadline: deadline.clone(),
                    snooze: None,
                    delivery: 1,
                    pending: None,
                });
                json!({"id":id,"delivery":1,"due_unix_ms":deadline.unix_ms,"local":deadline.local})
            }
            Request::Cancel { id, .. } => {
                let index = find(&next, *id)?;
                let delivery = next.items[index].delivery;
                next.items.remove(index);
                json!({"cancelled":true,"id":id,"delivery":delivery})
            }
            Request::Snooze {
                id,
                delivery,
                seconds,
                ..
            } => {
                let index = find_pending(&next, *id, *delivery)?;
                let until = time::resolve(&When::After { seconds: *seconds }, now, &self.zone)
                    .map_err(bad_time)?;
                let item = &mut next.items[index];
                item.delivery = item
                    .delivery
                    .checked_add(1)
                    .ok_or_else(|| err("full", "Delivery identifiers are exhausted."))?;
                item.snooze = Some(until.clone());
                item.pending = None;
                json!({"snoozed":true,"id":id,"delivery":item.delivery,"due_unix_ms":until.unix_ms,"local":until.local})
            }
            Request::Ack { id, delivery, .. } => {
                let index = find_pending(&next, *id, *delivery)?;
                let item = &mut next.items[index];
                match time::next_ahead(&item.deadline, now).map_err(bad_time)? {
                    Some(deadline) => {
                        item.delivery = item
                            .delivery
                            .checked_add(1)
                            .ok_or_else(|| err("full", "Delivery identifiers are exhausted."))?;
                        item.deadline = deadline;
                        item.pending = None;
                        item.snooze = None;
                    }
                    None => {
                        next.items.remove(index);
                    }
                }
                json!({"acknowledged":true,"id":id,"delivery":delivery})
            }
            _ => unreachable!("read-only requests returned above"),
        };
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or_else(|| err("full", "Schedule revision is exhausted."))?;
        let response = with_identity(body, &next.store_id, next.revision);
        next.last_receipt = Some(Receipt {
            request,
            response: response.clone(),
        });
        self.save(next)?;
        Ok(response)
    }

    fn guard(&self, guard: &Guard) -> Result<(), Error> {
        if guard.revision != self.state.revision {
            return Err(err(
                "conflict",
                "Schedule changed. Read its status and items before choosing another operation.",
            ));
        }
        Ok(())
    }
}

fn with_identity(mut body: Value, store_id: &str, revision: u64) -> Value {
    let object = body.as_object_mut().expect("response is an object");
    object.insert("ok".into(), true.into());
    object.insert("store_id".into(), store_id.into());
    object.insert("revision".into(), revision.into());
    body
}

fn find(state: &State, id: u64) -> Result<usize, Error> {
    state
        .items
        .iter()
        .position(|i| i.id == id)
        .ok_or_else(|| err("not_found", "That reminder or timer is no longer active."))
}
fn find_pending(state: &State, id: u64, delivery: u64) -> Result<usize, Error> {
    let index = find(state, id)?;
    let item = &state.items[index];
    if item.delivery != delivery || item.pending.is_none() {
        return Err(err(
            "stale_delivery",
            "That exact delivery is not pending; read the current item.",
        ));
    }
    Ok(index)
}
