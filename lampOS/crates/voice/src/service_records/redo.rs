//! Redo or edit a write from its record (`LAMP_SERVICE_REDO`, the
//! connector records plan, PR 1d): "do it again at 8 PM" after a cancelled
//! event, or "add Mai too", needs nothing repeated.
//!
//! Off unless the second worker has `LAMP_SERVICE_REDO=on` and
//! [`super::ENVIRONMENT`] on too; off, `service_write` is offered and
//! answered exactly as before.
//!
//! - **One field.** `service_write` takes [`FIELD`], a record key. The
//!   voice fills in every detail the record saved, then lays over it only
//!   what the call gave (each field whole), and the record's own service
//!   and action. Then the write goes on as any other: the services prepare
//!   it, the lamp reads it back in full, and only a plain "send" right
//!   after sends it ([`crate::services::gate`] unchanged).
//! - **Which records.** A `cancelled`, `failed`, `timed_out`, `refused` or
//!   still `prepared` write. A `done` mail is never changed: a follow-up
//!   mail to the same people is built from it (its subject `Re:`, the new
//!   text the call's), and the model is told to say plainly that a sent
//!   mail can't be changed ([`FOLLOW_UP_NOTE`]). A `done` calendar event is
//!   refused ([`EVENT_DONE`]): editing one is a separate action. Any other
//!   `done` write, and a read, is refused.
//! - **Linked.** The new record's `links` name the old one, and the old one
//!   gets `replaced_by`, both written to the memory ([`link`]).
//! - **Only the owner.** The fill runs inside the gate, after its owner,
//!   heard and seen checks ([`crate::services::OWNER_ONLY`]), and only the
//!   owner's own records are found. A record not held since the voice
//!   started is read from the Days memory first ([`bring`]), off the gate's
//!   lock, for the owner's line alone.
use super::{Ledger, Record, Status, kept, whose};
use serde_json::{Map, Value, json};

/// Only the exact value `on`, with [`super::ENVIRONMENT`], on the second
/// route.
pub const ENVIRONMENT: &str = "LAMP_SERVICE_REDO";

/// `service_write`'s field: the key of the record to redo.
pub const FIELD: &str = "from_record";

/// What the model is told of [`FIELD`].
pub const DESCRIPTION: &str = "To redo or change a past service write of theirs (cancelled, failed, timed out or refused; or a follow-up to a sent mail), such as \"do it again\" or \"redo it for Sunday\": its record key, the `key` shown with it in their past service actions or in what recall found. Give only what the person changed, each field whole; every other detail comes from the record. Read back and send as always.";

pub const NO_RECORD: &str = "Not done: no service action of theirs has that record key. Use a key from their past service actions, or ask for the whole write again.";
pub const A_READ: &str = "Not done: a search or read is not redone from a record. Call service_read again if they want it.";
pub const OTHER_KIND: &str = "Not done: that record is another service's or another action's. Keep the record's own service and action, or ask for a new write in full.";
pub const EVENT_DONE: &str = "Not done: that event is already on their calendar, and changing a created event is a separate action, not a redo. Say so plainly, in a few words.";
/// What a redo of a saved Gmail draft is told: it is changed or deleted by
/// its id (`LAMP_GMAIL_DRAFT_EDITS`), never saved again beside the old one
/// (a tester's lamp, 11 Nov).
pub const DRAFT_DONE: &str = "Not done: that draft is already saved in their Gmail Drafts. To change or delete it, call update_draft or delete_draft with its draft_id, the record's result_id, if those are listed; never save it again beside the old one. Otherwise say so plainly.";
pub const DONE_ALREADY: &str = "Not done: that was already done, and a done action is not redone from its record. Say so plainly; they can ask for a new one in full.";
pub const FOLLOW_UP_TEXT: &str = "Not done: that mail was already sent, and a sent mail can't be changed. Say so plainly, ask what a follow-up mail should say, and call again with from_record and that text.";
pub const CUT_TEXT: &str = "Not done: the saved text was too long to keep whole. Ask them for the whole text, and call again with from_record and that text.";
/// Told beside a follow-up mail's read-back.
pub const FOLLOW_UP_NOTE: &str = "That mail was already sent, and a sent mail can't be changed: this is a new follow-up mail to the same people. Say that plainly first, then the read_back.";

/// The most records asked of the memory for one not held here.
const ASKED_MOST: u64 = 50;
/// The longest the memory is waited for one.
#[cfg(not(test))]
const ASK_DEADLINE: std::time::Duration = std::time::Duration::from_millis(1_500);

#[cfg(not(test))]
fn own_on() -> bool {
    static ON: std::sync::LazyLock<bool> =
        std::sync::LazyLock::new(|| std::env::var(ENVIRONMENT).as_deref() == Ok("on"));
    *ON
}

#[cfg(test)]
thread_local! {
    static ON: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
fn own_on() -> bool {
    ON.with(std::cell::Cell::get)
}

/// Tests: redo on or off on this thread.
#[cfg(test)]
pub fn on_for_test(on: bool) {
    ON.with(|cell| cell.set(on));
}

/// Whether a write may be redone from a record: [`ENVIRONMENT`] and the
/// records both on.
pub fn on() -> bool {
    super::on() && own_on()
}

/// The record key a call names, when redo is on.
fn key_of(args: &Value) -> Option<String> {
    on().then_some(())?;
    args.get(FIELD)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(str::to_owned)
}

/// `person`'s record `key`, as held here.
fn held(person: &str, key: &str) -> Option<Record> {
    kept::ledger(|ledger| {
        ledger
            .records
            .iter()
            .find(|record| record.key == key && record.person == person)
            .cloned()
    })
}

fn status_of(wire: &str) -> Option<Status> {
    [
        Status::Prepared,
        Status::Done,
        Status::Cancelled,
        Status::Refused,
        Status::Failed,
        Status::TimedOut,
        Status::NoResult,
    ]
    .into_iter()
    .find(|status| status.wire() == wire)
}

/// One record as the memory lists it (`service_records`), for `person`.
fn from_memory(person: &str, row: &Value) -> Option<Record> {
    let text = |key: &str| row.get(key).and_then(Value::as_str).map(str::to_owned);
    Some(Record {
        key: text("key")?,
        person: person.to_owned(),
        service: text("service")?,
        action: text("action")?,
        status: status_of(row.get("status")?.as_str()?)?,
        reason: text("reason").unwrap_or_default(),
        detail: row
            .get("detail")
            .filter(|detail| detail.is_object())
            .cloned()
            .unwrap_or_else(|| json!({})),
        result_id: text("result_id").unwrap_or_default(),
        asked_ms: row["asked_unix_ms"].as_u64().unwrap_or(0),
        changed_ms: row["changed_unix_ms"].as_u64().unwrap_or(0),
        send_id: None,
        prepared_at: None,
        links: row["links"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        replaced_by: text("replaced_by").unwrap_or_default(),
    })
}

/// Before the gate, for the owner's line: a record the call names that is
/// not held here (made before the voice started) is asked of the memory
/// with `ask`, its exchange, and held. Never for anyone else's line.
pub fn bring_with(owner: Option<&str>, args: &Value, ask: impl FnOnce(&Value) -> Option<Value>) {
    let Some(key) = key_of(args) else {
        return;
    };
    let Some(person) = owner
        .filter(|owner| lamp_msgs::known::is_person_id(owner))
        .and_then(|owner| whose(Some(owner)))
    else {
        return;
    };
    if held(&person, &key).is_some() {
        return;
    }
    let line = json!({"op": "service_records", "person": person, "limit": ASKED_MOST});
    let found = ask(&line)
        .filter(|answer| answer["ok"] == true)
        .and_then(|answer| {
            answer["records"]
                .as_array()?
                .iter()
                .find(|row| row["key"] == key.as_str())
                .and_then(|row| from_memory(&person, row))
        });
    eprintln!(
        "lamp-voice: a redo's record {key} {} in the memory (LAMP_SERVICE_REDO)",
        if found.is_some() {
            "was found"
        } else {
            "was not found"
        }
    );
    if let Some(record) = found {
        kept::ledger(|ledger| ledger.add(record));
    }
}

/// [`bring_with`] on the Days memory's socket.
#[cfg(not(test))]
pub fn bring(owner: Option<&str>, args: &Value) {
    if key_of(args).is_none() {
        return;
    }
    let Some(socket) = crate::remember::days_socket() else {
        return;
    };
    bring_with(owner, args, |line| {
        crate::tools::exchange(&socket, line, ASK_DEADLINE)
    });
}

/// Whether `record` was a read.
fn a_read(record: &Record) -> bool {
    record.status == Status::NoResult
        || record.detail.get("found").is_some()
        || record.detail.get("what").is_some()
}

fn a_mail(record: &Record) -> bool {
    record.action == "send_mail"
}

/// `subject` as a reply's: `Re: ` once.
fn re(subject: &str) -> String {
    let subject = subject.trim();
    if subject.to_lowercase().starts_with("re:") {
        subject.to_owned()
    } else {
        format!("Re: {subject}")
    }
}

/// Inside the gate, once the owner's prepare passed every check: a write
/// that names [`FIELD`] is filled from `owner`'s record, then only what the
/// call gave laid over it; or why not, for the model. Off, or with no
/// [`FIELD`], `args` is left as it was.
pub fn fill(owner: &str, args: &mut Value) -> Result<(), &'static str> {
    let Some(key) = key_of(args) else {
        return Ok(());
    };
    let record = whose(Some(owner))
        .and_then(|person| held(&person, &key))
        .ok_or(NO_RECORD)?;
    let given = args.as_object().cloned().unwrap_or_default();
    if a_read(&record) {
        return Err(A_READ);
    }
    let same = |field: &str, kept: &str| {
        given
            .get(field)
            .and_then(Value::as_str)
            .is_none_or(|said| said.trim() == kept)
    };
    if !same("service", &record.service) || !same("action", &record.action) {
        return Err(OTHER_KIND);
    }
    let follow_up = record.status == Status::Done;
    if follow_up {
        if !a_mail(&record) {
            return Err(
                if record.service == "google_calendar" && record.action == "create_event" {
                    EVENT_DONE
                } else if record.service.starts_with("gmail")
                    && matches!(record.action.as_str(), "save_draft" | "update_draft")
                {
                    DRAFT_DONE
                } else {
                    DONE_ALREADY
                },
            );
        }
        if given.get("text").is_none_or(Value::is_null) {
            return Err(FOLLOW_UP_TEXT);
        }
    }
    if record.detail["cut"] == true && given.get("text").is_none_or(Value::is_null) {
        return Err(CUT_TEXT);
    }
    let mut filled: Map<String, Value> = record.detail.as_object().cloned().unwrap_or_default();
    filled.remove("cut");
    let mut changed = 0;
    for (field, value) in given {
        if matches!(field.as_str(), "service" | "action" | FIELD) {
            continue;
        }
        changed += 1;
        if value.is_null() {
            filled.remove(&field);
        } else {
            filled.insert(field, value);
        }
    }
    if follow_up && args.get("subject").is_none_or(Value::is_null) {
        let subject = re(filled
            .get("subject")
            .and_then(Value::as_str)
            .unwrap_or_default());
        filled.insert("subject".into(), json!(subject));
    }
    filled.insert("service".into(), json!(record.service));
    filled.insert("action".into(), json!(record.action));
    filled.insert(FIELD.into(), json!(key));
    eprintln!(
        "lamp-voice: a write filled from service record {key} ({}): {changed} field(s) changed{} (LAMP_SERVICE_REDO)",
        record.status.wire(),
        if follow_up { ", a follow-up mail" } else { "" }
    );
    *args = Value::Object(filled);
    Ok(())
}

/// A write prepared from the record its call names: `record`, new, links
/// back to it, and it is marked replaced by `record` (a still `prepared`
/// one cancelled too): the old one's change, to be written.
pub(super) fn link(ledger: &mut Ledger, record: &mut Record, args: &Value) -> Option<Record> {
    let key = key_of(args)?;
    let old = ledger
        .records
        .iter_mut()
        .find(|old| old.key == key && old.person == record.person)?;
    record.links = vec![key];
    old.replaced_by.clone_from(&record.key);
    // Settled only when it still waited; either way its change carries
    // `replaced_by`.
    let _ = Ledger::settle(old, Status::Cancelled, "redone as a new write", "");
    Some(old.clone())
}

/// What the model is told beside a read-back filled from a sent mail
/// ([`FOLLOW_UP_NOTE`]): `None` for any other write.
pub fn note(owner: &str, args: &Value) -> Option<&'static str> {
    let key = key_of(args)?;
    let record = held(&whose(Some(owner))?, &key)?;
    (record.status == Status::Done && a_mail(&record)).then_some(FOLLOW_UP_NOTE)
}

/// Tests: nothing is asked of a memory.
#[cfg(test)]
pub fn bring(_owner: Option<&str>, _args: &Value) {}

#[cfg(test)]
mod tests;
