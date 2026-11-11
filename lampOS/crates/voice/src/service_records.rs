//! Service records (`LAMP_SERVICE_RECORDS`, the connector records plan,
//! PR 1a): every outcome of a `service_read` or `service_write` the owner
//! asked for is written to the Days memory as one record, kept until it is
//! forgotten, so a later turn, a restart or another day can tell truthfully
//! whether it was done.
//!
//! Off unless the second worker has `LAMP_SERVICE_RECORDS=on` with the Days
//! memory; off, nothing is kept or written, and every send try's receipt
//! (#5853) is kept exactly as before.
//!
//! - **One source for sends.** The send receipts of #5853 (what
//!   [`crate::services::send_state`] tells a cut-in turn) are kept here, in
//!   the same ledger as the records, under one lock ([`keep_send`]): a send's
//!   answer sets its receipt and its record's status together, once. A turn
//!   cut while a send waits reads the receipt; the send's own call, which
//!   goes on to its answer, is the only writer of the record, so a send is
//!   never written twice, and never lost while the voice runs.
//! - **One record an action**, updated in place: `prepared` at the
//!   read-back, then `done`, `cancelled`, `refused`, `failed` or `timed_out`;
//!   a read is `done` or `no_result`. A status other than `prepared` is final.
//! - **What is kept.** The owner's own writes in full (every field the write
//!   was asked with: recipients, copies, subject, text, an event's fields).
//!   A read only its query and, of what it found, titles, senders, dates
//!   and links, with a short excerpt that has passed the services' own
//!   security check (`lamp_services::mail::is_security_mail` and
//!   `has_secret_shape`): anything that check hides is never stored.
//! - **Whose.** Saved under the owner's memory id alone (`p-` ids); with
//!   none known, nothing is saved. A refused call from someone who is not
//!   the owner keeps only its service, action, status and reason.
//! - **Never slows the reply.** A change is put on a small bounded queue
//!   ([`OUTBOX_MOST`]) and written by a thread of its own; memory down, it
//!   waits and is tried again, and the journal says so once. One journal
//!   line per record change: its key, service, action and status, never a
//!   word of what was asked or found.
//! - **Forgotten** with the person's everything ([`forgot`]): the ledger's
//!   records and every waiting change of theirs go; the send receipts stay,
//!   outcome only, so a cut-in turn still never sends twice.
use serde_json::{Map, Value, json};
use std::{
    collections::VecDeque,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// Only the exact value `on`, with the Days memory, on the second route.
pub const ENVIRONMENT: &str = "LAMP_SERVICE_RECORDS";

/// The records the voice holds, newest last: enough for every action a
/// day, and every waiting write. Older ones are in the memory.
pub const RECORDS_KEPT: usize = 64;
/// The most record changes waiting to be written.
pub const OUTBOX_MOST: usize = 32;
/// The longest one write to the memory waits.
#[cfg(not(test))]
const SAVE_DEADLINE: Duration = Duration::from_secs(2);
/// The longest wait before a write the memory did not answer is tried
/// again.
#[cfg(not(test))]
const RETRY_MOST: Duration = Duration::from_secs(30);
/// The most items of a read kept, and the longest excerpt of each.
pub const FOUND_MOST: usize = 10;
pub const EXCERPT_BYTES: usize = 160;
/// The most bytes of a write's detail, as JSON, under the memory's 16 KiB.
const DETAIL_BYTES: usize = 12_000;
/// The most bytes of a reason.
const REASON_BYTES: usize = 200;

/// A record's status, as the memory writes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Prepared,
    Done,
    Cancelled,
    Refused,
    Failed,
    TimedOut,
    NoResult,
}

impl Status {
    pub fn wire(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Done => "done",
            Self::Cancelled => "cancelled",
            Self::Refused => "refused",
            Self::Failed => "failed",
            Self::TimedOut => "timed_out",
            Self::NoResult => "no_result",
        }
    }
}

/// One record as the voice holds it.
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub key: String,
    pub person: String,
    pub service: String,
    pub action: String,
    pub status: Status,
    pub reason: String,
    pub detail: Value,
    pub result_id: String,
    pub asked_ms: u64,
    pub changed_ms: u64,
    /// The write's `send_id`, while it waits for a send.
    pub send_id: Option<String>,
    /// When it was prepared, for [`expire`].
    pub prepared_at: Option<Instant>,
    /// The records it was redone from (`LAMP_SERVICE_REDO`, [`redo`]).
    pub links: Vec<String>,
    /// The record redone from it, once one is ([`redo`]).
    pub replaced_by: String,
}

impl Record {
    /// The memory's `service_record` request for it.
    pub fn line(&self) -> Value {
        json!({
            "op": "service_record",
            "person": self.person,
            "key": self.key,
            "service": self.service,
            "action": self.action,
            "status": self.status.wire(),
            "reason": self.reason,
            "detail": self.detail,
            "result_id": self.result_id,
            "asked_unix_ms": self.asked_ms,
            "changed_unix_ms": self.changed_ms,
            "links": self.links,
            "replaced_by": self.replaced_by,
        })
    }
}

/// One send try's receipt (#5853): its attempt id, what it was told (none
/// while asked), and the record it went with.
#[derive(Clone, Debug)]
struct Receipt {
    attempt: u64,
    told: Option<Value>,
    record: Option<String>,
}

/// The receipts and the records, under one lock.
#[derive(Default)]
pub struct Ledger {
    receipts: VecDeque<Receipt>,
    records: VecDeque<Record>,
    made: u64,
    /// How many forgets ran (`forgot`, `forgot_about`).
    forgets: u64,
}

/// The record changes waiting to be written, oldest first: one a key, the
/// latest.
#[derive(Default)]
pub struct Outbox {
    waiting: VecDeque<Record>,
    /// Whether the memory did not answer the last write.
    down: bool,
}

#[cfg(not(test))]
mod kept {
    use super::{Ledger, Outbox};
    use std::sync::{Condvar, LazyLock, Mutex};

    static LEDGER: LazyLock<Mutex<Ledger>> = LazyLock::new(Mutex::default);
    pub static OUTBOX: LazyLock<(Mutex<Outbox>, Condvar)> =
        LazyLock::new(|| (Mutex::default(), Condvar::new()));

    pub fn ledger<R>(f: impl FnOnce(&mut Ledger) -> R) -> R {
        f(&mut LEDGER
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()))
    }

    pub fn outbox<R>(f: impl FnOnce(&mut Outbox) -> R) -> R {
        let result = f(&mut OUTBOX
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()));
        OUTBOX.1.notify_one();
        result
    }

    pub fn on() -> bool {
        static ON: LazyLock<bool> =
            LazyLock::new(|| std::env::var(super::ENVIRONMENT).as_deref() == Ok("on"));
        *ON
    }
}

/// Tests: each test thread its own, so parallel tests never meet.
#[cfg(test)]
mod kept {
    use super::{Ledger, Outbox};
    use std::cell::{Cell, RefCell};

    thread_local! {
        static LEDGER: RefCell<Ledger> = RefCell::default();
        static OUTBOX: RefCell<Outbox> = RefCell::default();
        pub static ON: Cell<bool> = const { Cell::new(false) };
    }

    pub fn ledger<R>(f: impl FnOnce(&mut Ledger) -> R) -> R {
        LEDGER.with(|cell| f(&mut cell.borrow_mut()))
    }

    pub fn outbox<R>(f: impl FnOnce(&mut Outbox) -> R) -> R {
        OUTBOX.with(|cell| f(&mut cell.borrow_mut()))
    }

    pub fn on() -> bool {
        ON.with(Cell::get)
    }
}

/// Whether records are kept ([`ENVIRONMENT`]).
pub fn on() -> bool {
    kept::on()
}

/// Tests: records on or off on this thread.
#[cfg(test)]
pub fn on_for_test(on: bool) {
    kept::ON.with(|cell| cell.set(on));
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

/// `text` on one line, at most `most` bytes, cut at a character.
fn tidy(text: &str, most: usize) -> String {
    let mut out = String::new();
    for c in text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
    {
        if out.len() + c.len_utf8() > most {
            break;
        }
        out.push(c);
    }
    out
}

/// The owner's memory id a record is saved under: `owner` when it is a
/// known person's, else the first known owner's; none, nothing is saved.
fn whose(owner: Option<&str>) -> Option<String> {
    owner
        .filter(|owner| lamp_msgs::known::is_person_id(owner))
        .map(str::to_owned)
        .or_else(|| {
            owner_ids()
                .into_iter()
                .find(|id| lamp_msgs::known::is_person_id(id))
        })
}

#[cfg(not(test))]
fn owner_ids() -> Vec<String> {
    crate::person::owner_ids()
}

#[cfg(test)]
fn owner_ids() -> Vec<String> {
    Vec::new()
}

/// What a write was asked with, for its record: every field but the
/// voice's and the gate's own, in full, cut only to fit the memory.
fn write_detail(args: &Value) -> Value {
    let mut detail = Map::new();
    if let Some(args) = args.as_object() {
        for (key, value) in args {
            let own = matches!(
                key.as_str(),
                "service" | "action" | "send_id" | "said" | crate::services::ATTEMPT
            );
            // The record redone from is a link, not a detail (`LAMP_SERVICE_REDO`).
            let redone_from = key == redo::FIELD && redo::on();
            if !own && !redone_from {
                detail.insert(key.clone(), value.clone());
            }
        }
    }
    let mut detail = Value::Object(detail);
    // Too long: the text is cut, whole words, and the cut said.
    while detail.to_string().len() > DETAIL_BYTES {
        let Some(text) = detail.get("text").and_then(Value::as_str) else {
            return json!({"cut": true});
        };
        let shorter = tidy(text, text.len() * 3 / 4);
        if shorter.len() >= text.len() {
            return json!({"cut": true});
        }
        detail["text"] = json!(shorter);
        detail["cut"] = json!(true);
    }
    detail
}

/// The first text of `item` under any of `keys`, a nested `dateTime` or
/// `date` or `email` included.
fn first_text(item: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        let value = item.get(*key)?;
        value
            .as_str()
            .or_else(|| {
                ["dateTime", "date", "email", "name"]
                    .iter()
                    .find_map(|inner| value.get(*inner).and_then(Value::as_str))
            })
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    })
}

/// What a read found, for its record: of each item at most its title,
/// sender, date, link and id, and a short excerpt only when the services'
/// security check passes it; an item the services left out is counted,
/// nothing of it kept.
pub fn found(told: &Value) -> (Vec<Value>, usize) {
    let mut found = Vec::new();
    let mut left_out = 0;
    for item in told
        .get("quoted")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if !item.is_object() {
            left_out += 1;
            continue;
        }
        if found.len() >= FOUND_MOST {
            continue;
        }
        let title = first_text(item, &["title", "subject", "name", "summary"]);
        let from = first_text(item, &["from", "sender", "organizer", "owner", "author"]);
        let date = first_text(
            item,
            &[
                "date",
                "start",
                "modifiedTime",
                "updated",
                "created",
                "createdTime",
            ],
        );
        let link = first_text(
            item,
            &["link", "url", "webViewLink", "htmlLink", "html_url"],
        );
        let id = first_text(item, &["id"]);
        let text = first_text(item, &["text", "snippet", "description", "body", "content"]);
        let mut kept = Map::new();
        let (from_said, title_said) = (
            from.clone().unwrap_or_default(),
            title.clone().unwrap_or_default(),
        );
        if lamp_services::mail::is_security_mail(&from_said, &title_said, "") {
            left_out += 1;
            continue;
        }
        // Each kept field held to the excerpt's rule (Thirteen's P2 on
        // #6073): a title or sender with a code's shape, or a sign-in,
        // reset or token link, is never stored.
        let title = title.filter(|title| !lamp_services::mail::has_code_shape(title));
        let from = from.filter(|from| !lamp_services::mail::from_has_code_shape(from));
        let link = link.filter(|link| !lamp_services::mail::is_secret_link(link));
        for (key, value) in [
            ("title", title),
            ("from", from),
            ("date", date),
            ("link", link),
            ("id", id),
        ] {
            if let Some(value) = value {
                kept.insert(key.into(), json!(tidy(&value, 300)));
            }
        }
        if let Some(text) = text {
            let excerpt = tidy(&text, EXCERPT_BYTES);
            let hidden = lamp_services::mail::is_security_mail(&from_said, &title_said, &text)
                || lamp_services::mail::has_secret_shape(&excerpt);
            if !hidden && !excerpt.is_empty() {
                kept.insert("excerpt".into(), json!(excerpt));
            }
        }
        found.push(Value::Object(kept));
    }
    (found, left_out)
}

/// What a read asked, for its record: what, the search words and filters.
fn read_detail(args: &Value) -> Map<String, Value> {
    let mut detail = Map::new();
    for key in [
        "what",
        "query",
        "id",
        "from",
        "to",
        "count",
        "repo",
        "calendar",
        "project",
        "workspace",
        "path",
        "tool",
    ] {
        if let Some(value) = args.get(key).filter(|value| !value.is_null()) {
            let value = match value.as_str() {
                Some(text) => json!(tidy(text, 300)),
                None => value.clone(),
            };
            detail.insert(key.into(), value);
        }
    }
    detail
}

fn text_of(args: &Value, key: &str) -> String {
    tidy(
        args.get(key).and_then(Value::as_str).unwrap_or_default(),
        64,
    )
}

impl Ledger {
    fn new_key(&mut self, now_ms: u64) -> String {
        self.made += 1;
        format!("v{now_ms}-{}", self.made)
    }

    fn add(&mut self, record: Record) -> Record {
        if self.records.len() >= RECORDS_KEPT {
            // A final one goes first; a waiting write's record last.
            let at = self
                .records
                .iter()
                .position(|record| record.status != Status::Prepared)
                .unwrap_or(0);
            if let Some(gone) = self.records.remove(at) {
                for receipt in &mut self.receipts {
                    if receipt.record.as_deref() == Some(gone.key.as_str()) {
                        receipt.record = None;
                    }
                }
            }
        }
        self.records.push_back(record.clone());
        record
    }

    fn by_send_id(&mut self, send_id: &str) -> Option<&mut Record> {
        self.records
            .iter_mut()
            .rev()
            .find(|record| record.send_id.as_deref() == Some(send_id))
    }

    fn by_key(&mut self, key: &str) -> Option<&mut Record> {
        self.records.iter_mut().find(|record| record.key == key)
    }

    /// `record` given `status` and `why`, if it still waits: the change.
    fn settle(record: &mut Record, status: Status, why: &str, result_id: &str) -> Option<Record> {
        if record.status != Status::Prepared || status == Status::Prepared {
            return None;
        }
        record.status = status;
        record.reason = tidy(why, REASON_BYTES);
        record.result_id = tidy(result_id, 512);
        record.changed_ms = unix_ms().max(record.asked_ms);
        record.send_id = None;
        record.prepared_at = None;
        Some(record.clone())
    }
}

/// The change written: put on the outbox, one a key (the latest), and
/// said in the journal, its key, service, action and status only.
fn save(record: Record) {
    eprintln!(
        "lamp-voice: service record {}: {} {} {} (LAMP_SERVICE_RECORDS)",
        record.key,
        record.service,
        record.action,
        record.status.wire()
    );
    let dropped = kept::outbox(|outbox| {
        if let Some(waiting) = outbox
            .waiting
            .iter_mut()
            .find(|waiting| waiting.key == record.key && waiting.person == record.person)
        {
            *waiting = record;
            return false;
        }
        let dropped = outbox.waiting.len() >= OUTBOX_MOST;
        if dropped {
            outbox.waiting.pop_front();
        }
        outbox.waiting.push_back(record);
        dropped
    });
    if dropped {
        eprintln!(
            "lamp-voice: a service record change was not saved: {OUTBOX_MOST} wait already (LAMP_SERVICE_RECORDS)"
        );
    }
    start_saver();
}

/// A write prepared and read back for `owner`, waiting for a send as
/// `send_id`: its record, `prepared`, with everything it was asked with.
pub fn prepared(owner: &str, service: &str, args: &Value, send_id: &str, now: Instant) {
    if !on() {
        return;
    }
    let Some(person) = whose(Some(owner)) else {
        return;
    };
    let at = unix_ms();
    let (record, redone) = kept::ledger(|ledger| {
        let key = ledger.new_key(at);
        let mut record = Record {
            key,
            person,
            service: tidy(service, 64),
            action: text_of(args, "action"),
            status: Status::Prepared,
            reason: String::new(),
            detail: write_detail(args),
            result_id: String::new(),
            asked_ms: at,
            changed_ms: at,
            send_id: Some(send_id.to_owned()),
            prepared_at: Some(now),
            links: Vec::new(),
            replaced_by: String::new(),
        };
        // Redone from a record (`LAMP_SERVICE_REDO`): linked both ways.
        let redone = redo::link(ledger, &mut record, args);
        (ledger.add(record), redone)
    });
    save(record);
    if let Some(redone) = redone {
        save(redone);
    }
}

/// The waiting write `send_id` let go, or answered: its record given
/// `status` and `why`, once.
pub fn settle(send_id: &str, status: Status, why: &str) {
    settle_with(send_id, status, why, "");
}

fn settle_with(send_id: &str, status: Status, why: &str, result_id: &str) {
    if !on() || send_id.is_empty() {
        return;
    }
    if let Some(change) = kept::ledger(|ledger| {
        ledger
            .by_send_id(send_id)
            .and_then(|record| Ledger::settle(record, status, why, result_id))
    }) {
        save(change);
    }
}

/// A call the gate or the services refused or failed before anything was
/// prepared: a record of its own, final at once. With `whole`, what it was
/// asked with too; without (someone not the owner, or not heard), only its
/// service, action, status and reason.
pub fn ended(
    owner: Option<&str>,
    name: &str,
    args: &Value,
    status: Status,
    why: &str,
    whole: bool,
) {
    if !on() || !crate::services::is_tool(name) || name == crate::services::CONNECTED {
        return;
    }
    let Some(person) = whose(owner) else {
        return;
    };
    let read = name == crate::services::READ;
    let detail = match (whole, read) {
        (false, _) => json!({}),
        (true, true) => Value::Object(read_detail(args)),
        (true, false) => write_detail(args),
    };
    let at = unix_ms();
    let record = kept::ledger(|ledger| {
        let key = ledger.new_key(at);
        ledger.add(Record {
            key,
            person,
            service: text_of(args, "service"),
            action: if read {
                text_of(args, "what")
            } else {
                text_of(args, "action")
            },
            status,
            reason: tidy(why, REASON_BYTES),
            detail,
            result_id: String::new(),
            asked_ms: at,
            changed_ms: at,
            send_id: None,
            prepared_at: None,
            links: Vec::new(),
            replaced_by: String::new(),
        })
    });
    save(record);
}

/// A read answered for `owner`: its query and what it found, `done`, or
/// `no_result` when it found nothing; an answered error is `failed`.
pub fn read(owner: &str, args: &Value, told: &Value) {
    if !on() {
        return;
    }
    if let Some(error) = told.get("error").and_then(Value::as_str) {
        ended(
            Some(owner),
            crate::services::READ,
            args,
            Status::Failed,
            error,
            true,
        );
        return;
    }
    let Some(person) = whose(Some(owner)) else {
        return;
    };
    let (items, left_out) = found(told);
    let mut detail = read_detail(args);
    let count = told
        .get("quoted")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    detail.insert("count".into(), json!(count));
    if left_out > 0 {
        detail.insert("left_out".into(), json!(left_out));
    }
    detail.insert("found".into(), json!(items));
    let at = unix_ms();
    let record = kept::ledger(|ledger| {
        let key = ledger.new_key(at);
        ledger.add(Record {
            key,
            person,
            service: text_of(args, "service"),
            action: text_of(args, "what"),
            status: if count == 0 {
                Status::NoResult
            } else {
                Status::Done
            },
            reason: String::new(),
            detail: Value::Object(detail),
            result_id: String::new(),
            asked_ms: at,
            changed_ms: at,
            send_id: None,
            prepared_at: None,
            links: Vec::new(),
            replaced_by: String::new(),
        })
    });
    save(record);
}

/// How far one send try got in `services::run` (#5853).
pub enum Got<'a> {
    /// Passed the gate and asked the service, no answer (yet).
    Asked,
    /// The gate's refusal.
    Refused(&'a Value),
    /// The service's answer.
    Answered(&'a Value),
    /// The service gave no answer within its bound: whether it went is not
    /// known.
    Unanswered,
}

/// One send try `attempt`, of the waiting write `send_id`, got as far as
/// `got`: its receipt (#5853) and its record together, under one lock. A
/// receipt that went (`sent:true`) is never undone; when full, the oldest
/// refusal goes, else the oldest try. With no attempt id (the Live route's
/// try) no receipt is kept, the record still is.
pub fn keep_send(attempt: Option<u64>, send_id: Option<&str>, got: Got<'_>) {
    let change = kept::ledger(|ledger| {
        let record = send_id
            .filter(|_| on())
            .and_then(|send_id| ledger.by_send_id(send_id))
            .map(|record| record.key.clone());
        if let Some(attempt) = attempt {
            let told = match &got {
                Got::Refused(told) | Got::Answered(told) => Some((*told).clone()),
                Got::Asked | Got::Unanswered => None,
            };
            let receipts = &mut ledger.receipts;
            if let Some(kept) = receipts.iter_mut().find(|kept| kept.attempt == attempt) {
                if kept
                    .told
                    .as_ref()
                    .is_none_or(|told| !crate::services::went(told))
                    && !matches!(got, Got::Unanswered)
                {
                    kept.told = told;
                }
                if kept.record.is_none() {
                    kept.record.clone_from(&record);
                }
            } else {
                if receipts.len() >= crate::services::RECEIPTS_KEPT {
                    let refused = receipts.iter().position(|kept| {
                        kept.told
                            .as_ref()
                            .is_some_and(|told| !crate::services::went(told))
                    });
                    receipts.remove(refused.unwrap_or(0));
                }
                receipts.push_back(Receipt {
                    attempt,
                    told,
                    record: record.clone(),
                });
            }
        }
        let record = ledger.by_key(record.as_deref()?)?;
        match got {
            Got::Asked | Got::Refused(_) => None,
            Got::Answered(told) if crate::services::went(told) => {
                // A saved Gmail draft's id (`draft12`) is its result id, so a
                // later change finds it from the record (`update_draft`).
                let id = first_text(
                    told,
                    &["id", "link", "url", "message_id", "event_id", "draft_id"],
                )
                .unwrap_or_default();
                // A Gmail draft is saved or deleted, never sent.
                let how = if told["sent"] == true {
                    "sent"
                } else if told["deleted"] == true {
                    "deleted from Drafts"
                } else {
                    "saved as a draft"
                };
                Ledger::settle(record, Status::Done, how, &id)
            }
            Got::Answered(told) if told["cancelled"] == true => {
                Ledger::settle(record, Status::Cancelled, "the service let it go", "")
            }
            Got::Answered(told) => Ledger::settle(
                record,
                Status::Failed,
                told.get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("not sent"),
                "",
            ),
            Got::Unanswered => Ledger::settle(
                record,
                Status::Failed,
                "the service did not answer in time, so whether it went is not known",
                "",
            ),
        }
    });
    if let Some(change) = change {
        save(change);
    }
}

/// The receipt of send try `attempt`: what it was told, `None` inside while
/// it was asked with no answer; `None` for a try never kept.
pub fn receipt(attempt: u64) -> Option<Option<Value>> {
    kept::ledger(|ledger| {
        ledger
            .receipts
            .iter()
            .find(|kept| kept.attempt == attempt)
            .map(|kept| kept.told.clone())
    })
}

/// A write prepared more than `within` before `now` and still waiting:
/// `timed_out`. Only the record: the waiting write itself is the gate's.
pub fn expire(now: Instant, within: Duration) {
    if !on() {
        return;
    }
    let changes: Vec<Record> = kept::ledger(|ledger| {
        ledger
            .records
            .iter_mut()
            .filter(|record| {
                record.status == Status::Prepared
                    && record
                        .prepared_at
                        .is_some_and(|at| now.saturating_duration_since(at) > within)
            })
            .filter_map(|record| {
                Ledger::settle(
                    record,
                    Status::TimedOut,
                    "no send was heard within two minutes of the read-back",
                    "",
                )
            })
            .collect()
    });
    changes.into_iter().for_each(save);
}

/// The memory forgot `person`'s everything, or everyone's (`None`): their
/// records and every change of theirs still waiting go. The receipts stay,
/// their outcome only, never what was asked.
pub fn forgot(person: Option<&str>) {
    let theirs = |record: &Record| person.is_none_or(|person| record.person == person);
    kept::ledger(|ledger| {
        let gone: Vec<String> = ledger
            .records
            .iter()
            .filter(|record| theirs(record))
            .map(|record| record.key.clone())
            .collect();
        ledger.records.retain(|record| !theirs(record));
        ledger.forgets += 1;
        for receipt in &mut ledger.receipts {
            if receipt
                .record
                .as_ref()
                .is_some_and(|key| gone.contains(key))
            {
                receipt.record = None;
            }
        }
    });
    kept::outbox(|outbox| outbox.waiting.retain(|record| !theirs(record)));
}

/// `text` lower case and without accents, its words.
fn words_in(text: &str) -> Vec<String> {
    use unicode_normalization::UnicodeNormalization;
    text.nfd()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .collect::<String>()
        .replace(['đ', 'Đ'], "d")
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(singular)
        .collect()
}

/// `word` as its singular, as the memory's forget compares it: "parties"
/// is "party", "events" "event".
fn singular(word: &str) -> String {
    match word.strip_suffix("ies") {
        Some(stem) if !stem.is_empty() => format!("{stem}y"),
        _ => word
            .strip_suffix('s')
            .filter(|stem| !stem.is_empty())
            .unwrap_or(word)
            .to_owned(),
    }
}

fn texts(value: &Value, out: &mut String) {
    match value {
        Value::String(text) => {
            out.push_str(text);
            out.push(' ');
        }
        Value::Array(items) => items.iter().for_each(|item| texts(item, out)),
        Value::Object(object) => object.values().for_each(|item| texts(item, out)),
        _ => {}
    }
}

/// Whether a record is `person`'s and has every key word of a subject of
/// `about` (each subject's key words), as the memory's forget matches it.
fn about_matches(person: &str, about: &[Vec<String>]) -> impl Fn(&Record) -> bool {
    let subjects: Vec<Vec<String>> = about
        .iter()
        .map(|words| words.iter().flat_map(|word| words_in(word)).collect())
        .collect();
    let person = person.to_owned();
    move |record: &Record| {
        let mut text = format!("{} {} {} ", record.service, record.action, record.reason);
        texts(&record.detail, &mut text);
        let words = words_in(&text);
        record.person == person
            && subjects.iter().any(|subject| {
                !subject.is_empty() && subject.iter().all(|word| words.contains(word))
            })
    }
}

/// A forget of `person`'s everything is about to be sent: every change of
/// theirs still waiting goes first, so the writer, even one in backoff,
/// never writes one after it (Jarvis's P2 on #6073). Their records held
/// go when the memory answers ([`forgot`]).
pub fn unsent(person: &str) {
    kept::outbox(|outbox| outbox.waiting.retain(|record| record.person != person));
}

/// A forget about `about` of `person`'s is about to be sent: the changes
/// still waiting that match it go first ([`unsent`]).
pub fn unsent_about(person: &str, about: &[Vec<String>]) {
    let matches = about_matches(person, about);
    kept::outbox(|outbox| outbox.waiting.retain(|record| !matches(record)));
}

/// The memory answered a forget about `about` (each subject's key words)
/// of `person`'s, whatever it removed: the records held here with every
/// key word of a subject go too, and their waiting changes, so none is
/// written again.
pub fn forgot_about(person: &str, about: &[Vec<String>]) {
    let matches = about_matches(person, about);
    let gone: Vec<String> = kept::ledger(|ledger| {
        let gone: Vec<String> = ledger
            .records
            .iter()
            .filter(|record| matches(record))
            .map(|record| record.key.clone())
            .collect();
        ledger.records.retain(|record| !gone.contains(&record.key));
        ledger.forgets += 1;
        for receipt in &mut ledger.receipts {
            if receipt
                .record
                .as_ref()
                .is_some_and(|key| gone.contains(key))
            {
                receipt.record = None;
            }
        }
        gone
    });
    kept::outbox(|outbox| {
        outbox
            .waiting
            .retain(|record| !gone.contains(&record.key) && !matches(record));
    });
}

/// The memory answered a forget about a mail contact of `person`'s that
/// removed `addresses`: the records held here sent to one of them go too,
/// and their waiting changes, so none is told or written again (step 5d on
/// lamp-06c1, 11 Nov).
pub fn forgot_sent_to(person: &str, addresses: &[String]) {
    if addresses.is_empty() {
        return;
    }
    let sent_to = |record: &Record| {
        record.person == person
            && recipients(&record.detail)
                .iter()
                .any(|address| addresses.contains(address))
    };
    let gone: Vec<String> = kept::ledger(|ledger| {
        let gone: Vec<String> = ledger
            .records
            .iter()
            .filter(|record| sent_to(record))
            .map(|record| record.key.clone())
            .collect();
        ledger.records.retain(|record| !gone.contains(&record.key));
        ledger.forgets += 1;
        for receipt in &mut ledger.receipts {
            if receipt
                .record
                .as_ref()
                .is_some_and(|key| gone.contains(key))
            {
                receipt.record = None;
            }
        }
        gone
    });
    kept::outbox(|outbox| {
        outbox
            .waiting
            .retain(|record| !gone.contains(&record.key) && !sent_to(record));
    });
}

/// The mail addresses a record's `detail` was sent to, as the memory reads
/// them: its `to`, `cc` and `bcc`, each a list or a comma-separated text,
/// `Name <address>` read as its address, lower case.
fn recipients(detail: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for key in ["to", "cc", "bcc"] {
        let entries: Vec<&str> = match detail.get(key) {
            Some(Value::String(text)) => text.split(',').collect(),
            Some(Value::Array(list)) => list.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        };
        for entry in entries {
            let entry = entry.trim();
            let address = match (entry.rfind('<'), entry.ends_with('>')) {
                (Some(open), true) => &entry[open + 1..entry.len() - 1],
                _ => entry,
            };
            let address = address.trim().to_lowercase();
            if !address.is_empty() {
                out.push(address);
            }
        }
    }
    out
}

/// How many forgets ran: the list after a start ([`told::merge`]) is
/// dropped when one ran since it was asked.
pub fn forgets() -> u64 {
    kept::ledger(|ledger| ledger.forgets)
}

/// The records held now, oldest first (tests and the note of PR 1b).
pub fn held() -> Vec<Record> {
    kept::ledger(|ledger| ledger.records.iter().cloned().collect())
}

/// The record changes waiting to be written, oldest first.
pub fn waiting() -> Vec<Record> {
    kept::outbox(|outbox| outbox.waiting.iter().cloned().collect())
}

/// What one try of [`save_next`] did.
#[derive(Debug, PartialEq, Eq)]
pub enum Saved {
    /// Nothing waits.
    Nothing,
    /// The oldest change was written, or refused for good (an older memory,
    /// a bad record): it no longer waits.
    Done,
    /// The memory did not answer: it waits, to be tried again.
    Down,
}

/// Writes the oldest waiting change with `send`, the memory's exchange:
/// written or refused, it goes; no answer, it stays first, and the journal
/// says so once until the memory answers again.
pub fn save_next(send: impl FnOnce(&Value) -> Option<Value>) -> Saved {
    let Some(record) = kept::outbox(|outbox| outbox.waiting.front().cloned()) else {
        return Saved::Nothing;
    };
    let answer = send(&record.line());
    kept::outbox(|outbox| {
        let Some(answer) = answer else {
            let waiting = outbox.waiting.len();
            if !outbox.down {
                eprintln!(
                    "lamp-voice: the memory did not take a service record: {waiting} wait, tried again (LAMP_SERVICE_RECORDS)"
                );
            }
            outbox.down = true;
            return Saved::Down;
        };
        if outbox.down {
            eprintln!(
                "lamp-voice: the memory takes service records again: {} wait (LAMP_SERVICE_RECORDS)",
                outbox.waiting.len()
            );
        }
        outbox.down = false;
        // The one sent, unless a forget took it or a newer change of it
        // came meanwhile: that is written next.
        if let Some(at) = outbox.waiting.iter().position(|waiting| *waiting == record) {
            outbox.waiting.remove(at);
        }
        if answer["ok"] != true {
            eprintln!(
                "lamp-voice: service record {} was refused by the memory: {} (LAMP_SERVICE_RECORDS)",
                record.key,
                answer["error"].as_str().unwrap_or("no reason")
            );
        }
        Saved::Done
    })
}

/// The writer: a thread of its own, started at the first change, that
/// writes each waiting change to the Days memory and marks a write waiting
/// too long `timed_out`; memory down, it tries again after 1 s, doubling to
/// [`RETRY_MOST`].
#[cfg(not(test))]
fn start_saver() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        let spawned = std::thread::Builder::new()
            .name("service-records".into())
            .spawn(|| {
                let mut wait = Duration::from_secs(1);
                loop {
                    {
                        let (lock, ready) = &*kept::OUTBOX;
                        let outbox = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                        let _ = ready
                            .wait_timeout(outbox, wait)
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                    }
                    expire(Instant::now(), crate::services::PENDING_FOR);
                    let Some(socket) = crate::remember::days_socket() else {
                        wait = RETRY_MOST;
                        continue;
                    };
                    loop {
                        match save_next(|line| {
                            crate::tools::exchange(&socket, line, SAVE_DEADLINE)
                        }) {
                            Saved::Done => wait = Duration::from_secs(1),
                            Saved::Nothing => break,
                            Saved::Down => {
                                wait = (wait * 2).min(RETRY_MOST);
                                break;
                            }
                        }
                    }
                }
            });
        if spawned.is_err() {
            eprintln!(
                "lamp-voice: the service records writer did not start; records are not saved (LAMP_SERVICE_RECORDS)"
            );
        }
    });
}

#[cfg(test)]
fn start_saver() {}

pub mod redo;
pub mod told;

#[cfg(test)]
mod tests;
