//! Durable scheduling policy. Every mutation is prepared on a copy, committed,
//! then made visible. No publication or speech is a storage acknowledgement.
use crate::{
    protocol::{Asker, Guard, MAX_PAGE_SIZE, MAX_REPLY_BYTES, Request},
    store::{MAX_STATE_BYTES, Store},
    time::{self, ClockSample, Deadline, When},
};
use lamp_local_time::LocalTime;
use lamp_msgs::{ClockStamp, ScheduleDue, ScheduleNotice};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fmt, io};

mod stored;

/// Optional stored metadata may be added without a bump. New enum values,
/// required fields, larger bounds, or fields whose omission could schedule
/// or deliver different work require a newer schema instead.
const SCHEMA: u32 = 1;

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
    /// The snapshot of the newest store kept aside, if any. Backends that
    /// can't set one aside have none.
    fn kept_snapshot(&self) -> io::Result<Option<Vec<u8>>> {
        Ok(None)
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
    fn kept_snapshot(&self) -> io::Result<Option<Vec<u8>>> {
        self.kept_snapshot()
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
struct Pending {
    overdue_at_detection_ms: u64,
    recovered_late: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Item {
    id: u64,
    text: String,
    created_unix_ms: i64,
    request_id: String,
    #[serde(deserialize_with = "stored::deadline")]
    deadline: Deadline,
    #[serde(default, deserialize_with = "stored::snooze")]
    snooze: Option<Deadline>,
    delivery: u64,
    pending: Option<Pending>,
    /// Who set it, as a memory person id (#4527's P2, (a)): optional stored
    /// metadata, so no `schema` bump. An item stored before it, or set with
    /// nobody sure, has none and belongs to the room ([`Asker::may_see`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    person: Option<String>,
}

impl Item {
    fn effective(&self) -> &Deadline {
        self.snooze.as_ref().unwrap_or(&self.deadline)
    }

    /// A timer: a duration from now (`set_timer`'s `After`), the only kind
    /// whose deadline keeps an elapsed clock. A snooze never changes the
    /// original deadline, so a snoozed timer stays a timer (#1595).
    fn timer(&self) -> bool {
        self.deadline.elapsed.is_some()
    }
}

/// Every line sees reminders and timers set with no sure person. A known
/// person's items remain theirs alone. No asker is the app or delivery
/// reader, which keeps its existing whole-store view.
fn visible(asker: Option<&Asker>, item: &Item) -> bool {
    asker.is_none_or(|asker| asker.may_see(item.person.as_deref()))
}

/// A setter's person id as memory makes them: `p-` and 16 hex digits.
fn valid_person(person: &str) -> bool {
    lamp_msgs::known::is_person_id(person) && person.len() <= 64
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Receipt {
    request: Request,
    response: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct State {
    schema: u32,
    store_id: String,
    revision: u64,
    next_id: u64,
    /// The reminders and timers this build reads. One it can't (a newer
    /// build's value, written without a `schema` bump, or a zone its time
    /// zone rules don't know) is in [`State::unread`].
    #[serde(deserialize_with = "stored::readable_items")]
    items: Vec<Item>,
    /// Reminders and timers in the store this build can't read, as their own
    /// JSON text and id: never sounded, snoozed or acknowledged here, and
    /// written back as they are, so the build that wrote them still finds
    /// them (#1680).
    #[serde(skip)]
    unread: Vec<Unread>,
    // Read separately: an unreadable retry cache must not hide valid items,
    // nor may unknown request fields be stripped to manufacture an exact retry.
    #[serde(skip_deserializing)]
    last_receipt: Option<Receipt>,
}

/// A stored reminder or timer this build can't read: its JSON text as
/// stored, and its `id` when that alone reads, to keep the items in order.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Unread {
    id: Option<u64>,
    json: String,
}

/// The state as stored: its items, read or not, in `id` order, each unread
/// one as its own text, byte for byte.
fn encode(state: &State) -> Result<Vec<u8>, Error> {
    if state.unread.is_empty() {
        return serde_json::to_vec(state).map_err(storage);
    }
    #[derive(Serialize)]
    #[serde(untagged)]
    enum Slot<'a> {
        Read(&'a Item),
        Unread(Box<serde_json::value::RawValue>),
    }
    #[derive(Serialize)]
    struct Out<'a> {
        #[serde(flatten)]
        rest: serde_json::Map<String, Value>,
        items: Vec<Slot<'a>>,
    }
    let Value::Object(mut rest) = serde_json::to_value(state).map_err(storage)? else {
        return Err(storage("state is not an object"));
    };
    rest.remove("items");
    let mut slots: Vec<(u64, Slot)> = state
        .items
        .iter()
        .map(|item| (item.id, Slot::Read(item)))
        .collect();
    for unread in &state.unread {
        let raw = serde_json::value::RawValue::from_string(unread.json.clone()).map_err(storage)?;
        slots.push((unread.id.unwrap_or(u64::MAX), Slot::Unread(raw)));
    }
    slots.sort_by_key(|(id, _)| *id);
    let items = slots.into_iter().map(|(_, slot)| slot).collect();
    serde_json::to_vec(&Out { rest, items }).map_err(storage)
}

/// The identity, revision and next id of a store a newer build wrote, read
/// from its top level as this build writes them, or `None` if any is missing
/// or not of this build's shape. Nothing else of it is read.
fn carried_identity(bytes: &[u8]) -> Option<(String, u64, u64)> {
    #[derive(Deserialize)]
    struct Top {
        store_id: String,
        revision: u64,
        next_id: u64,
    }
    let top: Top = serde_json::from_slice(bytes).ok()?;
    (valid_id(&top.store_id) && top.next_id >= 1 && top.revision < u64::MAX).then_some((
        top.store_id,
        top.revision,
        top.next_id,
    ))
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
        if self.schema != SCHEMA
            || !valid_id(&self.store_id)
            || self.next_id == 0
            || self.items.len() + self.unread.len() > MAX_ITEMS
            // An unread item's id is one this build never gives again.
            || self
                .unread
                .iter()
                .any(|unread| unread.id.is_some_and(|id| id == 0 || id >= self.next_id))
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
                || item.person.as_deref().is_some_and(|p| !valid_person(p))
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
                Request::Set { text, person, .. } => {
                    let item = self.items.last().ok_or_else(invalid)?;
                    if item.id != self.next_id - 1
                        || item.request_id != guard.request_id
                        || item.person != *person
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
                Request::Forget { person, .. } => {
                    if self
                        .items
                        .iter()
                        .any(|i| i.person.as_deref() == Some(person.as_str()))
                    {
                        return Err(invalid());
                    }
                    let forgotten = receipt.response["forgotten"].as_u64().ok_or_else(invalid)?;
                    json!({"forgotten":forgotten})
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
    set_aside: Option<String>,
    state: State,
    /// The zone a new reminder is set in, by name: `--zone`'s, or with
    /// `--zone system` the system's as setup last pointed it ([`Self::system`]).
    zone: String,
    /// `--zone system` (#820): the system's zone, named again at each request,
    /// so a re-setup is followed without a restart. A reminder already set
    /// keeps the zone it was set in.
    system: Option<LocalTime>,
    /// The zone asked for when it is not one this build knows, and
    /// [`Self::zone`] is the fallback (#1680).
    zone_asked: Option<String>,
    /// The fallback is UTC with no offset stored to stand for the lamp's
    /// zone: a new reminder at a clock time with no zone of its own is
    /// refused, as it would ring hours off (#1680).
    no_wall_clock: bool,
    last_clock: ClockSample,
    recovered: BTreeSet<u64>,
    failed: bool,
}

impl<P: Persistence> Engine<P> {
    /// `new_store_id` is fresh random data in serve, and injectable in tests.
    /// Existing stores retain their identity and monotonically rising IDs.
    ///
    /// A zone this build's time zone rules don't know never stops it: see
    /// [`Self::open_with_system`].
    pub fn open(
        persistence: P,
        zone: &str,
        now: &ClockSample,
        new_store_id: &str,
    ) -> Result<Self, Error> {
        Self::open_with_offset(persistence, zone, None, now, new_store_id)
    }

    /// [`Self::open`] with `offset_minutes`, the lamp's UTC offset east when
    /// setup ran (`LAMP_UTC_OFFSET_MINUTES` in `zone.env`), for the fallback
    /// from a zone this build doesn't know.
    pub fn open_with_offset(
        persistence: P,
        zone: &str,
        offset_minutes: Option<i32>,
        now: &ClockSample,
        new_store_id: &str,
    ) -> Result<Self, Error> {
        Self::open_with_system(
            persistence,
            zone,
            || LocalTime::zone("system"),
            offset_minutes,
            now,
            new_store_id,
        )
    }

    /// [`Self::open`] with `system` for the system's zone. A zone asked for
    /// that this build's time zone rules don't know (a newer tzdb's, in
    /// `zone.env`) falls back to the system's zone, followed as with `--zone
    /// system`. Setup points the system's zone at the same name, so that is
    /// unknown too, and the fallback is then the fixed offset
    /// `offset_minutes` (`Etc/GMT+3` for -180): 07:00 rings at 07:00 at the
    /// lamp, an hour off at most across summer time. With no offset stored,
    /// or one that is not whole hours, it is UTC, and a new reminder at a
    /// clock time with no zone of its own is refused, while one in minutes
    /// from now is set. A journal line and [`Self::zone_fallback`] for health
    /// say which (#1680). A reminder already set keeps ringing at the time it
    /// was set for, in its own zone.
    pub fn open_with_system(
        persistence: P,
        zone: &str,
        system: impl FnOnce() -> io::Result<LocalTime>,
        offset_minutes: Option<i32>,
        now: &ClockSample,
        new_store_id: &str,
    ) -> Result<Self, Error> {
        if zone != "system" && time::validate_zone(zone).is_ok() {
            return Self::open_in(persistence, zone, None, None, now, new_store_id);
        }
        let system = system()
            .map_err(|error| eprintln!("schedule: the system's time zone cannot be read: {error}"))
            .ok();
        Self::fall_back(persistence, zone, system, offset_minutes, now, new_store_id)
    }

    /// [`Self::open`] following `system`, the system's zone (`--zone system`).
    /// One whose name is not a zone the bundled zones know falls back to UTC
    /// until it names one, with a line (#1680).
    pub fn open_following(
        persistence: P,
        system: LocalTime,
        now: &ClockSample,
        new_store_id: &str,
    ) -> Result<Self, Error> {
        Self::fall_back(persistence, "system", Some(system), None, now, new_store_id)
    }

    /// Open in the system's zone when it names one this build knows, else
    /// in the fixed offset `offset_minutes`, else in UTC; `asked` is the zone
    /// asked for, said when it is not the one used.
    fn fall_back(
        persistence: P,
        asked: &str,
        system: Option<LocalTime>,
        offset_minutes: Option<i32>,
        now: &ClockSample,
        new_store_id: &str,
    ) -> Result<Self, Error> {
        let named = system
            .as_ref()
            .and_then(LocalTime::name)
            .filter(|name| time::validate_zone(name).is_ok());
        // `--zone system` with a system zone it knows is no fallback.
        let fell_back = named.is_none() || asked != "system";
        let fixed = offset_minutes.and_then(fixed_offset_zone);
        let no_wall_clock = named.is_none() && fixed.is_none();
        let zone = named
            .or_else(|| fixed.clone())
            .unwrap_or_else(|| "Etc/UTC".to_owned());
        let asked = fell_back.then(|| asked.to_owned());
        if let Some(asked) = &asked {
            let asked = if asked == "system" {
                "the system's zone"
            } else {
                asked
            };
            if no_wall_clock {
                eprintln!(
                    "schedule: {asked} is not a zone this build knows, and no whole-hour UTC offset is stored for the lamp; a new reminder at a clock time with no zone of its own is refused, one in minutes from now is set, and one already set rings at the time it was set for, in its own zone"
                );
            } else if fixed.as_deref() == Some(zone.as_str()) {
                eprintln!(
                    "schedule: {asked} is not a zone this build knows; a new reminder with no zone of its own is set in {zone} time, the lamp's UTC offset when setup ran (07:00 is 07:00 at the lamp, an hour off at most across summer time), and one already set rings at the time it was set for, in its own zone"
                );
            } else {
                eprintln!(
                    "schedule: {asked} is not a zone this build knows; a new reminder with no zone of its own is set in {zone} time (07:00 is 07:00 {zone}), and one already set rings at the time it was set for, in its own zone"
                );
            }
        }
        let mut engine = Self::open_in(persistence, &zone, asked, system, now, new_store_id)?;
        engine.no_wall_clock = no_wall_clock;
        Ok(engine)
    }

    fn open_in(
        mut persistence: P,
        zone: &str,
        zone_asked: Option<String>,
        system: Option<LocalTime>,
        now: &ClockSample,
        new_store_id: &str,
    ) -> Result<Self, Error> {
        time::validate_zone(zone).map_err(bad_time)?;
        now.check().map_err(bad_time)?;
        let bytes = persistence.load().map_err(storage)?;
        let mut set_aside = None;
        let mut carried = None;
        let mut dropped_receipt = false;
        let stored = match bytes {
            Some(bytes) if bytes.is_empty() || bytes.len() > MAX_STATE_BYTES => {
                return Err(storage("snapshot exceeds its byte bound"));
            }
            Some(bytes) => match stored::read(&bytes)? {
                stored::Read::Current(state, dropped) => {
                    dropped_receipt = dropped;
                    Some(*state)
                }
                stored::Read::Newer(schema) => {
                    // Validate before moving anything. The retained database
                    // is named by its schema and the second it was set aside,
                    // as lamp-tasks names its own (`newer-<schema>-<unix
                    // seconds>`), so a health reader can tell the newest.
                    if !valid_id(new_store_id) {
                        return Err(storage("invalid new store identity"));
                    }
                    let seconds = now.unix_ms.max(0) / 1000;
                    let kept = persistence
                        .set_aside(&format!("newer-{schema}-{seconds}"))
                        .map_err(storage)?;
                    set_aside = Some((schema, kept));
                    carried = carried_identity(&bytes);
                    None
                }
            },
            // An empty store with one kept aside beside it: a set-aside whose
            // successor was never committed (a crash between the rename and
            // the commit). Its identity is carried as if the set-aside had
            // just happened, so the ledger still follows (the reviewer's P3
            // on #1616). A kept store that can't be read leaves it fresh.
            None => {
                if let Some(kept) = persistence.kept_snapshot().ok().flatten() {
                    carried = carried_identity(&kept);
                    if carried.is_some() {
                        eprintln!(
                            "schedule: the store is empty and one set aside from a newer build is beside it; this store keeps its identity"
                        );
                    }
                }
                None
            }
        };
        let existing = stored.is_some();
        let state = match stored {
            Some(state) => state,
            // After a set-aside, the empty store keeps the set-aside one's
            // identity and goes on from its revision and ids where they can
            // be read, so the voice's delivery ledger, fenced to that identity,
            // follows it: a reminder set now is said, and one set aside is
            // gone as a deleted one is (the reviewer's P2 on #1562).
            None => match carried {
                Some((store_id, revision, next_id)) => State {
                    schema: SCHEMA,
                    store_id,
                    revision: revision + 1,
                    next_id,
                    items: Vec::new(),
                    unread: Vec::new(),
                    last_receipt: None,
                },
                None => State {
                    schema: SCHEMA,
                    store_id: new_store_id.into(),
                    revision: 0,
                    next_id: 1,
                    items: Vec::new(),
                    unread: Vec::new(),
                    last_receipt: None,
                },
            },
        };
        state.check()?;
        if !existing {
            persistence.commit(&encode(&state)?).map_err(storage)?;
        }
        if let Some((schema, kept)) = &set_aside {
            eprintln!(
                "schedule: storage was written by a newer build (schema {schema}, this build reads {SCHEMA}); it is set aside as {kept}, and this build starts with an empty store"
            );
        } else if dropped_receipt {
            eprintln!("schedule: the last mutation receipt is not one this build reads; dropped");
        }
        let recovered = state.items.iter().map(|item| item.id).collect();
        Ok(Self {
            persistence,
            set_aside: set_aside.map(|(_, name)| name),
            state,
            zone: zone.into(),
            zone_asked,
            no_wall_clock: false,
            system,
            last_clock: now.clone(),
            recovered,
            failed: false,
        })
    }

    /// Stored reminders and timers this build can't read, kept as they are
    /// and never sounded here (#1680).
    pub fn unread(&self) -> usize {
        self.state.unread.len()
    }

    /// The zone asked for and the one used instead, when the one asked for
    /// (`--zone`, or the system's with `--zone system`) is not one this
    /// build's time zone rules know (#1680). `None` when it is.
    pub fn zone_fallback(&self) -> Option<(&str, &str)> {
        self.zone_asked
            .as_deref()
            .map(|asked| (asked, self.zone.as_str()))
    }

    /// The retained database's name when opening a newer schema started a
    /// fresh store. Nothing from the retained store is delivered or merged.
    pub fn set_aside(&self) -> Option<&str> {
        self.set_aside.as_deref()
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
        let bytes = encode(&candidate)?;
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
            "kind": if item.timer() { "timer" } else { "reminder" },
            "request_id": item.request_id,
            "state": if item.pending.is_some() { "pending" } else if remaining.is_none() { "waiting_for_clock" } else { "scheduled" },
            "due_unix_ms": Self::shown(item, now).unix_ms, "local": Self::shown(item, now).local,
            "original_due_unix_ms": item.deadline.unix_ms,
            "repeat": item.deadline.repeat, "snoozed": item.snooze.is_some(),
            "remaining_ms": remaining.map(|r| r.max(0)), "late": late, "late_by_ms": late_by_ms
        })
    }

    /// [`Self::item_view`] with when it was set ([`Request::ListSet`]), in
    /// the lamp's zone (`time::set_at`): null when that can't be said,
    /// which the model is told as unknown.
    fn item_view_set(&self, item: &Item, now: &ClockSample) -> Value {
        let mut view = self.item_view(item, now);
        let set = time::set_at(item.created_unix_ms, now, &self.zone).ok();
        view["set_local"] = json!(set.as_ref().map(|(local, _)| local));
        view["set_day"] = json!(set.as_ref().map(|(_, day)| day));
        view
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
        // `ListSet` is `List` whose items also say when each was set.
        // With an asker, only the items that line may see (#4527's P2).
        let (request, with_set, asker) = match request {
            Request::ListSet {
                after_id,
                limit,
                pending_only,
                asker,
            } => (
                Request::List {
                    after_id,
                    limit,
                    pending_only,
                },
                true,
                asker,
            ),
            request => (request, false, None),
        };
        self.follow_zone();
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
                    .filter(|item| {
                        item.id > *after_id
                            && (!pending_only || item.pending.is_some())
                            && visible(asker.as_ref(), item)
                    })
                    .take(usize::from(*limit) + 1)
                    .collect();
                let mut views = Vec::new();
                for item in available.iter().take(usize::from(*limit)) {
                    views.push(if with_set {
                        self.item_view_set(item, now)
                    } else {
                        self.item_view(item, now)
                    });
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
                let mut body = json!({"items":views,"next_after_id":next_after_id});
                if let Some(asker) = &asker {
                    // What this line may see that is set and not yet due,
                    // never the whole store's count (#1595, #4527's P2).
                    body["not_yet_due"] = json!(
                        self.state
                            .items
                            .iter()
                            .filter(|i| i.pending.is_none() && visible(Some(asker), i))
                            .count()
                    );
                }
                return Ok(self.reply(body));
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
            Request::Set {
                text, when, person, ..
            } => {
                if person.as_deref().is_some_and(|p| !valid_person(p)) {
                    return Err(err("bad_request", "The setter is not a person id."));
                }
                if !valid_text(text) {
                    return Err(err(
                        "bad_text",
                        "Reminder text must contain 1 to 300 UTF-8 bytes and no control characters.",
                    ));
                }
                // Items this build can't read count too: they stay.
                if next.items.len() + next.unread.len() >= MAX_ITEMS {
                    return Err(err(
                        "full",
                        "There are already 128 active reminders and timers; cancel or acknowledge one.",
                    ));
                }
                if self.no_wall_clock
                    && matches!(
                        when,
                        When::At { zone: None, .. }
                            | When::Daily { zone: None, .. }
                            | When::Weekly { zone: None, .. }
                    )
                {
                    return Err(err(
                        "bad_time",
                        "This lamp's zone is not one this build knows and no UTC offset is stored, so a reminder at a clock time can't be set; one in minutes from now, or at a time in a named zone, can.",
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
                    person: person.clone(),
                });
                json!({"id":id,"delivery":1,"due_unix_ms":deadline.unix_ms,"local":deadline.local})
            }
            Request::Cancel { id, asker, .. } => {
                let index = find_for(&next, *id, asker.as_ref())?;
                let delivery = next.items[index].delivery;
                next.items.remove(index);
                json!({"cancelled":true,"id":id,"delivery":delivery})
            }
            Request::Snooze {
                id,
                delivery,
                seconds,
                asker,
                ..
            } => {
                find_for(&next, *id, asker.as_ref())?;
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
            Request::Forget { person, .. } => {
                if !valid_person(person) {
                    return Err(err("bad_request", "That is not a person id."));
                }
                let before = next.items.len();
                next.items
                    .retain(|i| i.person.as_deref() != Some(person.as_str()));
                json!({"forgotten":before - next.items.len()})
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

    /// With `--zone system`, the system's zone as it is now; one that names
    /// no zone this build knows keeps the last that did.
    fn follow_zone(&mut self) {
        if let Some(name) = self.system.as_ref().and_then(LocalTime::name)
            && name != self.zone
            && time::validate_zone(&name).is_ok()
        {
            eprintln!(
                "schedule: the system's zone is now {name} (was {})",
                self.zone
            );
            self.zone = name;
            self.no_wall_clock = false;
            // `--zone system` naming a zone it knows again is no fallback.
            if self.zone_asked.as_deref() == Some("system") {
                self.zone_asked = None;
            }
        }
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

/// The tzdb's fixed-offset zone for `minutes` east of UTC (`Etc/GMT+3` for
/// -180: its sign is POSIX's, west positive), when the offset is whole hours
/// and such a zone exists.
fn fixed_offset_zone(minutes: i32) -> Option<String> {
    if minutes % 60 != 0 {
        return None;
    }
    let west = -(minutes / 60);
    let name = match west {
        0 => "Etc/GMT".to_owned(),
        west => format!("Etc/GMT{west:+}"),
    };
    time::validate_zone(&name).is_ok().then_some(name)
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
/// [`find`] for a line that may see its own and the room's items ([`Asker::may_see`]): one
/// it may not see is not found, never named as someone else's.
fn find_for(state: &State, id: u64, asker: Option<&Asker>) -> Result<usize, Error> {
    let index = find(state, id)?;
    if !visible(asker, &state.items[index]) {
        return Err(err(
            "not_found",
            "That reminder or timer is no longer active.",
        ));
    }
    Ok(index)
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
