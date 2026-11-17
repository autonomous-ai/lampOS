//! Service records (the voice's `LAMP_SERVICE_RECORDS`, the connector
//! records plan, PR 1a): one row for each thing the owner asked of a linked
//! service (a mail sent, an event created, a Drive search), with what was
//! asked, its status and why, so a later turn, a restart or a new day can
//! tell truthfully whether it was done.
//!
//! - **One row an action**, keyed by the voice's own `key`, updated in place
//!   ([`Days::service_record_put`]): `prepared` becomes `done`, `cancelled`,
//!   `refused`, `failed` or `timed_out`; a read is `done` or `no_result`. A
//!   status other than `prepared` is final: a later put of the same key
//!   keeps it and says which it kept.
//! - **A known person's alone** (`p-` ids): the owner's. Never the room's, a
//!   rules scope's or a guest's.
//! - **Kept until forgotten.** In a table of its own, so the day's expiry,
//!   unused-entry expiry, the tidy and the nightly pass never reach it
//!   (they work on `entries`). Forget everything and forgetting the person
//!   take them all ([`empty`]); a forget about a thing takes the records
//!   that name every key word of it ([`Days::service_records_forget`]).
//! - **Bounded.** At most [`MOST_PER_PERSON`] a person (the oldest change
//!   goes first past it), [`DETAIL_BYTES`] of detail each.
//! - **A source kind** (the linked-service memory plan, slice 1): `voice`,
//!   the connector records the voice writes today, or a linked kind
//!   (`task`, `agent`, `reminder`) that names its source in `source_ref`
//!   (`{service, id, session?, computer?}`, at most [`SOURCE_REF_BYTES`])
//!   with a one-line `summary` (at most [`SUMMARY_BYTES`]). Each linked
//!   kind keeps at most [`MOST_PER_KIND`] a person, inside the
//!   [`MOST_PER_PERSON`], its oldest change going first. A put that names
//!   none of them is a `voice` record, as before.
//! - **A forget wins by the deed's time** (`deed_unix_ms`): a put whose
//!   deed or ask is before the person's last forget is refused.
//!
//! What a read found is stored by the voice as metadata and a short masked
//! excerpt only; this store keeps what it is given and checks only sizes
//! and shapes.
use crate::{
    days::{Days, Error, Result, ensure_person},
    store::{self, Person},
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde_json::{Value, json};
use unicode_normalization::UnicodeNormalization;

/// A record's statuses, as the wire and the table write them.
pub const STATUSES: [&str; 7] = [
    "prepared",
    "done",
    "cancelled",
    "refused",
    "failed",
    "timed_out",
    "no_result",
];

/// The most records one person keeps: years of asks. Past it the record
/// changed longest ago goes.
pub const MOST_PER_PERSON: usize = 10_000;
/// The most bytes of one record's detail, as JSON.
pub const DETAIL_BYTES: usize = 16_384;
/// The most bytes of a reason, a service, an action and a result id.
const REASON_BYTES: usize = 200;
const NAME_BYTES: usize = 64;
const RESULT_ID_BYTES: usize = 512;
/// The most records one list gives.
pub const LIST_MOST: usize = 50;
/// The most links a record has to the records it came from.
const LINKS_MOST: usize = 8;
/// The most forgets about a thing held to refuse a record asked before
/// one ([`Days::service_records_forget`]).
pub const FORGOT_ABOUT_HELD: usize = 64;
/// A record's source kinds, as the wire and the table write them: the
/// voice's connector records, then the linked kinds; `sdk` is a paired
/// computer's daily count of its notes' changes (tracker row C157), a
/// count only ([`sdk_count_ok`]).
pub const SOURCE_KINDS: [&str; 5] = ["voice", "task", "agent", "reminder", "sdk"];
/// The most records of one linked kind (not `voice`) a person keeps,
/// inside [`MOST_PER_PERSON`]. Past it that kind's record changed longest
/// ago goes.
pub const MOST_PER_KIND: usize = 2_000;
/// The most bytes of a record's `source_ref`, as JSON.
pub const SOURCE_REF_BYTES: usize = 512;
/// The most bytes of a record's one-line summary, cut at a whole character.
pub const SUMMARY_BYTES: usize = 160;
/// The fields a `source_ref` may have; `service` and `id` are required.
const SOURCE_REF_FIELDS: [&str; 4] = ["service", "id", "session", "computer"];

/// The columns the linked-service memory plan (slice 1) adds to
/// `service_records`, each added once by [`migrate`]: the first `ALTER`
/// of `days.sql`'s tables. An older build names its columns on every
/// insert and select, so it passes these over (a row it writes is a
/// `voice` one), and its forget still deletes the whole row, pointer
/// included.
const ADDED_COLUMNS: [(&str, &str); 4] = [
    (
        "source_kind",
        "ALTER TABLE service_records ADD COLUMN source_kind TEXT NOT NULL DEFAULT 'voice' CHECK(source_kind IN ('voice','task','agent','reminder','sdk'))",
    ),
    (
        "source_ref",
        "ALTER TABLE service_records ADD COLUMN source_ref TEXT NOT NULL DEFAULT '{}'",
    ),
    (
        "summary",
        "ALTER TABLE service_records ADD COLUMN summary TEXT NOT NULL DEFAULT ''",
    ),
    (
        "deed_unix_ms",
        "ALTER TABLE service_records ADD COLUMN deed_unix_ms INTEGER",
    ),
];

/// Adds [`ADDED_COLUMNS`] the table lacks (`pragma table_info`), widens a
/// slice-1 `source_kind` CHECK to take `sdk` ([`allow_sdk`]), then makes
/// their index, in one transaction: run at every open after `days.sql`,
/// so it is idempotent; `user_version` stays 1.
pub(crate) fn migrate(db: &mut Connection) -> Result<()> {
    let tx = db.transaction()?;
    let have: Vec<String> = {
        let mut statement = tx.prepare("SELECT name FROM pragma_table_info('service_records')")?;
        statement
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (column, alter) in ADDED_COLUMNS {
        if !have.iter().any(|name| name == column) {
            tx.execute_batch(alter)?;
        }
    }
    allow_sdk(&tx)?;
    tx.execute_batch(
        "CREATE INDEX IF NOT EXISTS service_records_kind ON service_records(person,source_kind,changed_unix_ms);",
    )?;
    tx.commit()?;
    Ok(())
}

/// The `source_kind` CHECK slice 1 (`1dfa0be70a`) added, without `sdk`,
/// as SQLite stores it in the table's `sql`, and the one that replaces it.
const SLICE_1_KIND_CHECK: &str = "CHECK(source_kind IN ('voice','task','agent','reminder'))";
const KIND_CHECK: &str = "CHECK(source_kind IN ('voice','task','agent','reminder','sdk'))";

/// A table that opened on slice 1 keeps that slice's `source_kind` CHECK,
/// which refuses every `sdk` row (AppOnly's P2 on #7574): SQLite cannot
/// change a CHECK in place, so the table is rebuilt inside `tx`, the way
/// SQLite's own guide does it: a new table from the stored `sql` with the
/// CHECK widened, every row copied, the old one dropped and the new one
/// renamed, then its indexes and triggers made again from their stored
/// `sql`. The rows are counted before and after, and any difference
/// fails the open (the transaction is rolled back). A table whose `sql`
/// has no slice-1 CHECK (a fresh one, or one already rebuilt) is left as
/// it is.
fn allow_sdk(tx: &Transaction) -> Result<()> {
    let table: String = tx.query_row(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name='service_records'",
        [],
        |r| r.get(0),
    )?;
    if !table.contains(SLICE_1_KIND_CHECK) {
        return Ok(());
    }
    let rebuilt = table.replacen(SLICE_1_KIND_CHECK, KIND_CHECK, 1).replacen(
        "service_records",
        "service_records_rebuilt",
        1,
    );
    let parts: Vec<String> = {
        let mut statement = tx.prepare(
            "SELECT sql FROM sqlite_master WHERE tbl_name='service_records' AND type IN ('index','trigger') AND sql IS NOT NULL ORDER BY type,name",
        )?;
        statement
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let count = |tx: &Transaction, table: &str| -> Result<u64> {
        Ok(tx.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?)
    };
    let before = count(tx, "service_records")?;
    tx.execute_batch("DROP TABLE IF EXISTS service_records_rebuilt")?;
    tx.execute_batch(&rebuilt)?;
    tx.execute_batch("INSERT INTO service_records_rebuilt SELECT * FROM service_records")?;
    if count(tx, "service_records_rebuilt")? != before {
        return Err(Error::new(
            "storage",
            "the service records rebuild lost rows",
        ));
    }
    tx.execute_batch(
        "DROP TABLE service_records; ALTER TABLE service_records_rebuilt RENAME TO service_records;",
    )?;
    for part in parts {
        tx.execute_batch(&part)?;
    }
    if count(tx, "service_records")? != before {
        return Err(Error::new(
            "storage",
            "the service records rebuild lost rows",
        ));
    }
    Ok(())
}

/// One record as the voice puts it.
#[derive(Debug, Clone)]
pub struct RecordIn {
    pub key: String,
    pub service: String,
    pub action: String,
    pub status: String,
    pub reason: String,
    pub detail: Option<Value>,
    pub result_id: String,
    pub asked_unix_ms: u64,
    pub changed_unix_ms: u64,
    pub links: Vec<String>,
    /// The key of the record that replaced this one (a redo), or empty.
    pub replaced_by: String,
    /// One of [`SOURCE_KINDS`]; none is `voice` (every put before the
    /// linked kinds). A record keeps the kind it was made with.
    pub source_kind: Option<String>,
    /// Where a linked record's thing lives (`{service, id, session?,
    /// computer?}`): required for a linked kind; none keeps what is held.
    pub source_ref: Option<Value>,
    /// A one-line summary, tidied to [`SUMMARY_BYTES`]; none keeps what is
    /// held.
    pub summary: Option<String>,
    /// When the deed was done at its service; none keeps what is held (an
    /// older row has none). A forget after it refuses the put.
    pub deed_unix_ms: Option<u64>,
}

/// How far ahead of memory's own clock a linked record's deed may be:
/// a deed later than that could pass a forget made after it arrived.
pub const DEED_AHEAD_MS: u64 = 5_000;

/// One record as `linked.sock` takes it (the linked-service memory plan,
/// slice 1b): an agent's read in a skill run, sent by lamp-services. It
/// names no person: memory finds the owner itself, from the task row the
/// voice wrote for that run ([`Days::linked_record_put`]).
#[derive(Debug, Clone)]
pub struct LinkedIn {
    /// Required; `agent` only (the voice writes tasks and reminders).
    pub source_kind: String,
    /// The key of the task row (`t-…`) whose `source_ref.session` is `run`.
    pub task: String,
    /// The skill run the read was made in.
    pub run: String,
    /// The read's number in its run: the record's key is `a-<run>-<n>`.
    pub n: u32,
    pub action: String,
    pub status: String,
    pub deed_unix_ms: u64,
    pub summary: String,
    /// `{service, id, session?, computer?}`; its `service` is the record's.
    pub source_ref: Value,
}

/// `record`'s kind, checked.
fn kind_of(record: &RecordIn) -> Result<&str> {
    let kind = record.source_kind.as_deref().unwrap_or("voice");
    if SOURCE_KINDS.contains(&kind) {
        Ok(kind)
    } else {
        Err(Error::new(
            "bad_kind",
            "the record's source kind is unknown",
        ))
    }
}

/// An `sdk` record is a count and nothing else (tracker row C157, the
/// first builder's yes of 17 Nov): service `sdk`, action `notes_changed`,
/// status `done`, with no detail, reason, result id or links, so no key or
/// value of a computer's notes can ever be stored under it; its summary is
/// the voice's one line of the count.
fn sdk_count_ok(record: &RecordIn) -> Result<()> {
    let empty_detail = match &record.detail {
        None | Some(Value::Null) => true,
        Some(Value::Object(object)) => object.is_empty(),
        Some(_) => false,
    };
    if record.service == "sdk"
        && record.action == "notes_changed"
        && record.status == "done"
        && empty_detail
        && record.reason.is_empty()
        && record.result_id.is_empty()
        && record.links.is_empty()
        && record.replaced_by.is_empty()
    {
        Ok(())
    } else {
        Err(Error::new(
            "bad_detail",
            "an sdk record is a count only: service sdk, action notes_changed, status done, nothing else",
        ))
    }
}

/// `record`'s `source_ref` as stored, checked: an object of
/// [`SOURCE_REF_FIELDS`] only, texts without control characters, a
/// non-empty `service` and `id`, at most [`SOURCE_REF_BYTES`]. A linked
/// kind must name one; a `voice` record may. None: keep what is held.
fn source_ref_of(kind: &str, value: Option<&Value>) -> Result<Option<String>> {
    let bad = || {
        Err(Error::new(
            "bad_source_ref",
            "a linked record's source_ref is an object of a service and an id (and a session and a computer), at most 512 bytes",
        ))
    };
    let object = match value {
        None | Some(Value::Null) if kind == "voice" => return Ok(None),
        Some(Value::Object(object)) => object,
        _ => return bad(),
    };
    let texts_ok = object.iter().all(|(field, value)| {
        SOURCE_REF_FIELDS.contains(&field.as_str())
            && value
                .as_str()
                .is_some_and(|text| !text.chars().any(char::is_control))
    });
    let named = |field: &str| {
        object
            .get(field)
            .and_then(Value::as_str)
            .is_some_and(|text| !text.trim().is_empty())
    };
    let text = Value::Object(object.clone()).to_string();
    if !texts_ok || !named("service") || !named("id") || text.len() > SOURCE_REF_BYTES {
        return bad();
    }
    Ok(Some(text))
}

fn key_ok(key: &str) -> bool {
    (1..=64).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub(crate) fn owner_only(person: &Person) -> Result<()> {
    if lamp_msgs::known::is_person_id(person.as_str()) {
        Ok(())
    } else {
        Err(Error::new(
            "bad_person",
            "only a known person has service records",
        ))
    }
}

/// `text` lower case, without accents, for a forget's match.
pub(crate) fn folded(text: &str) -> String {
    text.nfd()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .collect::<String>()
        .replace(['đ', 'Đ'], "d")
        .to_lowercase()
}

fn words_of(value: &Value, out: &mut String) {
    match value {
        Value::String(text) => {
            out.push_str(text);
            out.push(' ');
        }
        Value::Array(items) => items.iter().for_each(|item| words_of(item, out)),
        Value::Object(object) => object.values().for_each(|item| words_of(item, out)),
        _ => {}
    }
}

/// `word` as its singular: "parties" is "party", "events" "event".
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

/// Whether `word` is one of `words`, or its plural or singular.
pub(crate) fn has_word(words: &[&str], word: &str) -> bool {
    let word = singular(word);
    words.iter().any(|said| singular(said) == word)
}

/// `about` (each subject's words) folded, the empty words and subjects
/// left out.
pub(crate) fn subjects_of(about: &[Vec<String>]) -> Vec<Vec<String>> {
    about
        .iter()
        .map(|words| {
            words
                .iter()
                .map(|word| folded(word.trim()))
                .filter(|word| !word.is_empty())
                .collect::<Vec<_>>()
        })
        .filter(|words| !words.is_empty())
        .collect()
}

/// Whether a record whose words are `text` (its service, action, reason,
/// result id and every text of its detail) has every word of a subject.
pub(crate) fn about_any(subjects: &[Vec<String>], text: &str) -> bool {
    let text = folded(text);
    let words: Vec<&str> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    subjects
        .iter()
        .any(|subject| subject.iter().all(|word| has_word(&words, word)))
}

/// The record's words, as a forget about a thing matches them: its
/// service, action, reason, result id, summary and every text of its
/// detail.
fn text_of(
    service: &str,
    action: &str,
    reason: &str,
    result_id: &str,
    summary: &str,
    detail: &Value,
) -> String {
    let mut text = format!("{service} {action} {reason} {result_id} {summary} ");
    words_of(detail, &mut text);
    text
}

/// A stored record's words before its detail, as [`text_of`] has them, in
/// SQL: what a recall's and a forget's search read.
const TEXT_SQL: &str = "service||' '||action||' '||reason||' '||result_id||' '||summary";

/// The mail addresses a record's `detail` was sent to: its `to`, `cc`
/// and `bcc`, each a list or a comma-separated text, an entry's
/// `Name <address>` read as its address, lower case.
pub(crate) fn recipients(detail: &Value) -> Vec<String> {
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

/// A contact forget removed `addresses` of `person`'s: every record of
/// theirs sent to one of them goes in the same transaction. How many went.
pub(crate) fn forget_sent_to(
    tx: &Transaction<'_>,
    person: &Person,
    addresses: &[String],
) -> Result<u64> {
    if addresses.is_empty() {
        return Ok(0);
    }
    let rows: Vec<(String, String)> = {
        let mut statement =
            tx.prepare("SELECT key, detail FROM service_records WHERE person=?1")?;
        statement
            .query_map([person.as_str()], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut removed = 0;
    for (key, detail) in rows {
        let detail = serde_json::from_str::<Value>(&detail).unwrap_or(Value::Null);
        if recipients(&detail)
            .iter()
            .any(|address| addresses.contains(address))
        {
            removed += tx.execute(
                "DELETE FROM service_records WHERE person=?1 AND key=?2",
                params![person.as_str(), key],
            )? as u64;
        }
    }
    Ok(removed)
}

/// Forget everything of `person` (or everyone's, `None`) at `now`: a
/// record asked before it is refused from now on ([`Days::service_record_put`]).
/// Only a known person's own, or everyone's, is marked.
pub(crate) fn mark_forgot(tx: &Transaction<'_>, person: Option<&Person>, now: u64) -> Result<()> {
    let who = match person {
        None => "",
        Some(person) if lamp_msgs::known::is_person_id(person.as_str()) => person.as_str(),
        Some(_) => return Ok(()),
    };
    tx.execute(
        "INSERT INTO service_records_forgot(person,at_unix_ms) VALUES(?1,?2) ON CONFLICT(person) DO UPDATE SET at_unix_ms=max(at_unix_ms,?2)",
        params![who, now],
    )?;
    Ok(())
}

impl Days {
    /// Puts `record` for `person`: new, or the same key updated in place.
    /// A record no longer `prepared` keeps its status and fields; the
    /// status kept is answered.
    pub fn service_record_put(&mut self, person: &Person, record: &RecordIn) -> Result<String> {
        owner_only(person)?;
        if !key_ok(&record.key) {
            return Err(Error::new(
                "bad_key",
                "a record key is 1 to 64 of A-Z, a-z, 0-9, _ and -",
            ));
        }
        if !STATUSES.contains(&record.status.as_str()) {
            return Err(Error::new("bad_status", "the record status is unknown"));
        }
        let service = store::tidy(&record.service, NAME_BYTES);
        let action = store::tidy(&record.action, NAME_BYTES);
        if service.is_empty() || action.is_empty() {
            return Err(Error::new(
                "empty",
                "a record names its service and its action",
            ));
        }
        let detail = match &record.detail {
            None | Some(Value::Null) => "{}".to_owned(),
            Some(detail @ Value::Object(_)) => detail.to_string(),
            Some(_) => {
                return Err(Error::new("bad_detail", "a record's detail is an object"));
            }
        };
        if detail.len() > DETAIL_BYTES {
            return Err(Error::new(
                "too_long",
                "a record's detail is at most 16 KiB",
            ));
        }
        if record.links.len() > LINKS_MOST || !record.links.iter().all(|link| key_ok(link)) {
            return Err(Error::new(
                "bad_key",
                "a record links at most 8 record keys",
            ));
        }
        if !record.replaced_by.is_empty() && !key_ok(&record.replaced_by) {
            return Err(Error::new("bad_key", "replaced_by is a record key"));
        }
        let reason = store::tidy(&record.reason, REASON_BYTES);
        let result_id = store::tidy(&record.result_id, RESULT_ID_BYTES);
        let kind = kind_of(record)?;
        if kind == "sdk" {
            sdk_count_ok(record)?;
        }
        let source_ref = source_ref_of(kind, record.source_ref.as_ref())?;
        let summary = record
            .summary
            .as_deref()
            .map(|summary| store::tidy(summary, SUMMARY_BYTES));
        let deed = record
            .deed_unix_ms
            .map(i64::try_from)
            .transpose()
            .map_err(|_| Error::new("bad_time", "the deed's time is out of range"))?;
        self.refuse_forgotten(person, record, || {
            text_of(
                &service,
                &action,
                &reason,
                &result_id,
                summary.as_deref().unwrap_or(""),
                record.detail.as_ref().unwrap_or(&Value::Null),
            )
        })?;
        let links = json!(record.links).to_string();
        let tx = self.db.transaction()?;
        ensure_person(&tx, person)?;
        let kept: Option<(String, String)> = tx
            .query_row(
                "SELECT status, source_kind FROM service_records WHERE person=?1 AND key=?2",
                params![person.as_str(), record.key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if kept.as_ref().is_some_and(|(_, held)| held != kind) {
            return Err(Error::new(
                "bad_kind",
                "a record keeps the source kind it was made with",
            ));
        }
        let status = match kept.map(|(status, _)| status) {
            // A day's count: its later puts carry the day's higher count,
            // so its summary, pointer, deed and changed time follow them.
            Some(kept) if kind == "sdk" => {
                tx.execute(
                    "UPDATE service_records SET changed_unix_ms=max(changed_unix_ms,?3),source_ref=coalesce(?4,source_ref),summary=coalesce(?5,summary),deed_unix_ms=coalesce(?6,deed_unix_ms) WHERE person=?1 AND key=?2",
                    params![
                        person.as_str(),
                        record.key,
                        record.changed_unix_ms,
                        source_ref,
                        summary,
                        deed
                    ],
                )?;
                kept
            }
            Some(kept) if kept != "prepared" => {
                // Final: only who replaced it may still be said.
                if !record.replaced_by.is_empty() {
                    tx.execute(
                        "UPDATE service_records SET replaced_by=?3 WHERE person=?1 AND key=?2",
                        params![person.as_str(), record.key, record.replaced_by],
                    )?;
                }
                kept
            }
            Some(_) => {
                tx.execute(
                    "UPDATE service_records SET service=?3,action=?4,status=?5,reason=?6,detail=?7,result_id=?8,changed_unix_ms=max(changed_unix_ms,?9),links=?10,replaced_by=?11,source_ref=coalesce(?12,source_ref),summary=coalesce(?13,summary),deed_unix_ms=coalesce(?14,deed_unix_ms) WHERE person=?1 AND key=?2",
                    params![
                        person.as_str(),
                        record.key,
                        service,
                        action,
                        record.status,
                        reason,
                        detail,
                        result_id,
                        record.changed_unix_ms,
                        links,
                        record.replaced_by,
                        source_ref,
                        summary,
                        deed
                    ],
                )?;
                record.status.clone()
            }
            None => {
                tx.execute(
                    "INSERT INTO service_records(person,key,service,action,status,reason,detail,result_id,asked_unix_ms,changed_unix_ms,links,replaced_by,source_kind,source_ref,summary,deed_unix_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
                    params![
                        person.as_str(),
                        record.key,
                        service,
                        action,
                        record.status,
                        reason,
                        detail,
                        result_id,
                        record.asked_unix_ms,
                        record.changed_unix_ms.max(record.asked_unix_ms),
                        links,
                        record.replaced_by,
                        kind,
                        source_ref.as_deref().unwrap_or("{}"),
                        summary.as_deref().unwrap_or(""),
                        deed
                    ],
                )?;
                trim_kind(&tx, person, kind)?;
                trim(&tx, person)?;
                record.status.clone()
            }
        };
        tx.commit()?;
        Ok(status)
    }

    /// Puts an agent's record from `linked.sock` under the owner memory
    /// finds itself (the first builder's condition 1): the one known
    /// person holding a `task` row keyed `record.task` whose
    /// `source_ref.session` is `record.run`. No such row, or two people's,
    /// is `no_owner`, and nothing is kept. The record goes through
    /// [`Self::service_record_put`], so a forget wins by its deed
    /// (condition 2) and its summary and pointer are bounded (condition
    /// 5); a deed more than [`DEED_AHEAD_MS`] ahead of `now` is refused.
    /// The record's key and the status kept.
    pub fn linked_record_put(&mut self, record: &LinkedIn, now: u64) -> Result<(String, String)> {
        if record.source_kind != "agent" {
            return Err(Error::new(
                "bad_kind",
                "a linked record from this socket is an agent's",
            ));
        }
        // A linked record says what was done: a blank summary leaves a
        // forget about a thing no words to match (El's P2 on #7463).
        if store::tidy(&record.summary, SUMMARY_BYTES).is_empty() {
            return Err(Error::new(
                "empty",
                "a linked record says what was done in its summary",
            ));
        }
        let key = format!("a-{}-{}", record.run, record.n);
        if !key_ok(&record.run)
            || !key_ok(&key)
            || !key_ok(&record.task)
            || !record.task.starts_with("t-")
        {
            return Err(Error::new(
                "bad_key",
                "a linked record names a task row's key (t-…) and a run that make a record key",
            ));
        }
        if record.deed_unix_ms > now.saturating_add(DEED_AHEAD_MS) {
            return Err(Error::new(
                "bad_time",
                "the deed's time is ahead of memory's clock",
            ));
        }
        let service = record
            .source_ref
            .get("service")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        if service.trim().is_empty() {
            return Err(Error::new(
                "bad_source_ref",
                "a linked record's source_ref names its service",
            ));
        }
        let owners: Vec<(String, String)> = {
            let mut statement = self.db.prepare(
                "SELECT person, source_ref FROM service_records WHERE key=?1 AND source_kind='task'",
            )?;
            statement
                .query_map([record.task.as_str()], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let owner = match owners.as_slice() {
            [(person, source_ref)]
                if lamp_msgs::known::is_person_id(person)
                    && serde_json::from_str::<Value>(source_ref)
                        .ok()
                        .and_then(|source| source.get("session")?.as_str().map(str::to_owned))
                        .is_some_and(|session| session == record.run) =>
            {
                Person::new(person)
            }
            _ => None,
        }
        .ok_or_else(|| {
            Error::new(
                "no_owner",
                "no known person's task row matches this task and run",
            )
        })?;
        let status = self.service_record_put(
            &owner,
            &RecordIn {
                key: key.clone(),
                service,
                action: record.action.clone(),
                status: record.status.clone(),
                reason: String::new(),
                detail: None,
                result_id: String::new(),
                asked_unix_ms: record.deed_unix_ms,
                changed_unix_ms: now.max(record.deed_unix_ms),
                links: vec![record.task.clone()],
                replaced_by: String::new(),
                source_kind: Some("agent".to_owned()),
                source_ref: Some(record.source_ref.clone()),
                summary: Some(record.summary.clone()),
                deed_unix_ms: Some(record.deed_unix_ms),
            },
        )?;
        Ok((key, status))
    }

    /// A record asked before the latest forget of `person`'s everything
    /// (or everyone's), or before a forget about a thing it matches, is
    /// refused: a change the voice sent before the forget and this store
    /// took after it is never kept (Jarvis's P2 on #6073). A record whose
    /// deed (`deed_unix_ms`) is before the forget is refused the same way,
    /// whenever it was asked: a forget wins by the deed's time (the first
    /// builder's condition 2 for linked records). `text`: the record's
    /// words.
    fn refuse_forgotten(
        &self,
        person: &Person,
        record: &RecordIn,
        text: impl FnOnce() -> String,
    ) -> Result<()> {
        let refused = || {
            Err(Error::new(
                "forgotten",
                "the record was asked before a forget that takes it",
            ))
        };
        // The earlier of its ask and its deed.
        let since = record
            .deed_unix_ms
            .map_or(record.asked_unix_ms, |deed| deed.min(record.asked_unix_ms));
        let mark: Option<i64> = self.db.query_row(
            "SELECT max(at_unix_ms) FROM service_records_forgot WHERE person IN (?1,'')",
            [person.as_str()],
            |r| r.get(0),
        )?;
        if mark.is_some_and(|mark| u64::try_from(mark).unwrap_or(0) > since) {
            return refused();
        }
        let about: Vec<&Vec<Vec<String>>> = self
            .records_forgot_about
            .iter()
            .filter(|(whose, _, at)| whose == person.as_str() && *at > since)
            .map(|(_, subjects, _)| subjects)
            .collect();
        // Sent to an address a contact forget removed (step 5d).
        let sent_to = recipients(record.detail.as_ref().unwrap_or(&Value::Null));
        if !sent_to.is_empty()
            && self
                .records_forgot_addresses
                .iter()
                .any(|(whose, addresses, at)| {
                    whose == person.as_str()
                        && *at > since
                        && addresses.iter().any(|address| sent_to.contains(address))
                })
        {
            return refused();
        }
        if about.is_empty() {
            return Ok(());
        }
        // An agent's record with no summary, asked before a forget about a
        // thing: with no words to match, it may be the very record that
        // forget took, so it is refused (El's P2 on #7463).
        if record.source_kind.as_deref() == Some("agent")
            && record
                .summary
                .as_deref()
                .is_none_or(|summary| store::tidy(summary, SUMMARY_BYTES).is_empty())
        {
            return refused();
        }
        let text = text();
        if about.iter().any(|subjects| about_any(subjects, &text)) {
            return refused();
        }
        Ok(())
    }

    /// `person`'s records, the latest change first, at most `limit` (and
    /// [`LIST_MOST`]).
    pub fn service_records(&self, person: &Person, limit: usize) -> Result<Vec<Value>> {
        owner_only(person)?;
        let mut statement = self.db.prepare(&format!(
            "SELECT {COLUMNS} FROM service_records WHERE person=?1 ORDER BY changed_unix_ms DESC, asked_unix_ms DESC, key LIMIT ?2"
        ))?;
        let rows = statement.query_map(
            params![person.as_str(), limit.min(LIST_MOST) as i64],
            listed,
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// `person`'s record of `key`, whatever its age: none, or one (the
    /// voice's `record_source`, the linked-service memory plan, slice 7).
    /// Another person's key is none, as a key never kept is.
    pub fn service_record_keyed(&self, person: &Person, key: &str) -> Result<Vec<Value>> {
        owner_only(person)?;
        let mut statement = self.db.prepare(&format!(
            "SELECT {COLUMNS} FROM service_records WHERE person=?1 AND key=?2"
        ))?;
        let rows = statement.query_map(params![person.as_str(), key], listed)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// `person`'s records whose words match `about` (a recall's words, the
    /// voice's `LAMP_SERVICE_RECORDS_TOLD`), whatever their age: the most
    /// words matched first, then the latest change; at most `limit` (and
    /// [`LIST_MOST`]). A word matches a record's word lower case, without
    /// accents, as its singular, or as the start of a longer one
    /// ("octavia" finds `octaviab@example.com`); small words ("the",
    /// "that", "did") match nothing.
    pub fn service_records_about(
        &self,
        person: &Person,
        about: &str,
        limit: usize,
    ) -> Result<Vec<Value>> {
        owner_only(person)?;
        let wanted: Vec<String> = folded(about)
            .split(|c: char| !c.is_alphanumeric())
            .filter(|word| word.len() > 1 && !SMALL_WORDS.contains(word))
            .map(|word| singular(if word == "email" { "mail" } else { word }))
            .collect();
        if wanted.is_empty() {
            return Ok(Vec::new());
        }
        let mut scored: Vec<(usize, i64, i64, String)> = {
            let mut statement = self.db.prepare(
                &format!("SELECT key, {TEXT_SQL}, detail, changed_unix_ms, asked_unix_ms FROM service_records WHERE person=?1"),
            )?;
            let rows = statement.query_map([person.as_str()], |r| {
                let mut text: String = r.get(1)?;
                let detail: String = r.get(2)?;
                text.push(' ');
                if let Ok(detail) = serde_json::from_str::<Value>(&detail) {
                    words_of(&detail, &mut text);
                }
                Ok((
                    r.get::<_, String>(0)?,
                    text,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                ))
            })?;
            let mut scored = Vec::new();
            for row in rows {
                let (key, text, changed, asked) = row?;
                let text = folded(&text);
                let words: Vec<String> = text
                    .split(|c: char| !c.is_alphanumeric())
                    .filter(|word| !word.is_empty())
                    .map(singular)
                    .collect();
                let score = wanted
                    .iter()
                    .filter(|want| {
                        words.iter().any(|word| {
                            word.as_str() == want.as_str()
                                || (want.len() >= 4 && word.starts_with(want.as_str()))
                        })
                    })
                    .count();
                if score > 0 {
                    scored.push((score, changed, asked, key));
                }
            }
            scored
        };
        scored.sort_by(|a, b| (b.0, b.1, b.2, &a.3).cmp(&(a.0, a.1, a.2, &b.3)));
        scored.truncate(limit.min(LIST_MOST));
        let mut found = Vec::new();
        for (_, _, _, key) in scored {
            let mut statement = self.db.prepare_cached(&format!(
                "SELECT {COLUMNS} FROM service_records WHERE person=?1 AND key=?2"
            ))?;
            if let Some(record) = statement
                .query_row(params![person.as_str(), key], listed)
                .optional()?
            {
                found.push(record);
            }
        }
        Ok(found)
    }

    /// The records of `person` about every key word of any of `about`
    /// (each a subject's words, as the voice's forget gives them),
    /// compared lower case and without accents, removed: how many. A
    /// record's words are its service, action, reason and every text of
    /// its detail. Nothing at the service is undone.
    /// Every service record of `person` removed (`service_records_forget`
    /// with `all`), as forget everything removes them, and a put asked
    /// before it refused: the voice's forget everything for the services'
    /// owner id when it is not the speaker's own (the private room).
    pub fn service_records_forget_all(&mut self, person: &Person) -> Result<u64> {
        owner_only(person)?;
        let tx = self.db.transaction()?;
        let removed = empty(&tx, Some(person))?;
        mark_forgot(&tx, Some(person), self.now)?;
        tx.commit()?;
        Ok(removed)
    }

    pub fn service_records_forget(
        &mut self,
        person: &Person,
        about: &[Vec<String>],
    ) -> Result<u64> {
        owner_only(person)?;
        let subjects = subjects_of(about);
        if subjects.is_empty() {
            return Ok(0);
        }
        // Held first, found or not: a record asked before it that matches
        // and comes after it is refused ([`Self::refuse_forgotten`]).
        if self.records_forgot_about.len() >= FORGOT_ABOUT_HELD {
            self.records_forgot_about.pop_front();
        }
        self.records_forgot_about.push_back((
            person.as_str().to_owned(),
            subjects.clone(),
            self.now,
        ));
        let tx = self.db.transaction()?;
        let rows: Vec<(String, String)> = {
            let mut statement = tx.prepare(&format!(
                "SELECT key, {TEXT_SQL}, detail FROM service_records WHERE person=?1"
            ))?;
            statement
                .query_map([person.as_str()], |r| {
                    let mut text: String = r.get(1)?;
                    let detail: String = r.get(2)?;
                    text.push(' ');
                    if let Ok(detail) = serde_json::from_str::<Value>(&detail) {
                        words_of(&detail, &mut text);
                    }
                    Ok((r.get(0)?, text))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let mut removed = 0;
        for (key, text) in rows {
            if about_any(&subjects, &text) {
                removed += tx.execute(
                    "DELETE FROM service_records WHERE person=?1 AND key=?2",
                    params![person.as_str(), key],
                )? as u64;
            }
        }
        tx.commit()?;
        Ok(removed)
    }
}

/// A record's columns, as [`listed`] reads them.
const COLUMNS: &str = "key,service,action,status,reason,detail,result_id,asked_unix_ms,changed_unix_ms,links,replaced_by,source_kind,source_ref,summary,deed_unix_ms";

/// One record of [`COLUMNS`], as a list gives it.
fn listed(r: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let detail: String = r.get(5)?;
    let links: String = r.get(9)?;
    Ok(json!({
        "key": r.get::<_, String>(0)?,
        "service": r.get::<_, String>(1)?,
        "action": r.get::<_, String>(2)?,
        "status": r.get::<_, String>(3)?,
        "reason": r.get::<_, String>(4)?,
        "detail": serde_json::from_str::<Value>(&detail).unwrap_or_else(|_| json!({})),
        "result_id": r.get::<_, String>(6)?,
        "asked_unix_ms": r.get::<_, i64>(7)?,
        "changed_unix_ms": r.get::<_, i64>(8)?,
        "links": serde_json::from_str::<Value>(&links).unwrap_or_else(|_| json!([])),
        "replaced_by": r.get::<_, String>(10)?,
        "source_kind": r.get::<_, String>(11)?,
        "source_ref": serde_json::from_str::<Value>(&r.get::<_, String>(12)?).unwrap_or_else(|_| json!({})),
        "summary": r.get::<_, String>(13)?,
        "deed_unix_ms": r.get::<_, Option<i64>>(14)?,
    }))
}

/// Words a recall's search of the records passes over: they would match
/// every record.
const SMALL_WORDS: [&str; 52] = [
    "the", "an", "that", "this", "these", "those", "to", "for", "of", "my", "me", "you", "your",
    "did", "do", "does", "it", "its", "about", "with", "on", "in", "at", "what", "which",
    "happened", "and", "or", "is", "was", "were", "are", "be", "from", "we", "our", "us", "he",
    "she", "they", "them", "there", "any", "some", "again", "please", "lamp", "hey", "ok", "okay",
    "one", "thing",
];

/// Past [`MOST_PER_PERSON`], `person`'s record changed longest ago goes.
fn trim(tx: &Transaction<'_>, person: &Person) -> Result<()> {
    tx.execute(
        "DELETE FROM service_records WHERE person=?1 AND key IN (SELECT key FROM service_records WHERE person=?1 ORDER BY changed_unix_ms DESC, asked_unix_ms DESC, key LIMIT -1 OFFSET ?2)",
        params![person.as_str(), MOST_PER_PERSON as i64],
    )?;
    Ok(())
}

/// Past [`MOST_PER_KIND`], `person`'s record of linked `kind` changed
/// longest ago goes; another kind's never. A `voice` record is bounded by
/// [`MOST_PER_PERSON`] alone.
fn trim_kind(tx: &Transaction<'_>, person: &Person, kind: &str) -> Result<()> {
    if kind == "voice" {
        return Ok(());
    }
    tx.execute(
        "DELETE FROM service_records WHERE person=?1 AND source_kind=?2 AND key IN (SELECT key FROM service_records WHERE person=?1 AND source_kind=?2 ORDER BY changed_unix_ms DESC, asked_unix_ms DESC, key LIMIT -1 OFFSET ?3)",
        params![person.as_str(), kind, MOST_PER_KIND as i64],
    )?;
    Ok(())
}

/// Forget everything (`forget_all`, `forget_person`): `person`'s records,
/// or everyone's with `None`. How many went.
pub(crate) fn empty(tx: &Transaction<'_>, person: Option<&Person>) -> Result<u64> {
    Ok(tx.execute(
        "DELETE FROM service_records WHERE ?1 IS NULL OR person=?1",
        [person.map(Person::as_str)],
    )? as u64)
}

#[cfg(test)]
mod tests {
    use crate::{
        days::{DAY_MS, Days},
        days_protocol::answer,
        store::{LocalTime, Options},
    };
    use serde_json::{Value, json};
    use std::{fs, path::PathBuf};

    const NOW: u64 = 1_791_892_800_000;
    const OWNER: &str = "p-0123456789abcdef";
    const OTHER: &str = "p-fedcba9876543210";

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> (Self, Days) {
            let dir = std::env::temp_dir().join(format!(
                "lamp-service-records-{}-{name}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&dir);
            let scratch = Self(dir);
            let store = scratch.open();
            (scratch, store)
        }

        fn open(&self) -> Days {
            Days::open(
                Options {
                    dir: self.0.clone(),
                    keep_days: 30,
                    local: LocalTime::Offset(0),
                },
                NOW,
            )
            .unwrap()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn send_at(store: &mut Days, request: Value, now: u64) -> Value {
        serde_json::from_str(&answer(store, request.to_string().as_bytes(), now).line).unwrap()
    }

    fn send(store: &mut Days, request: Value) -> Value {
        send_at(store, request, NOW)
    }

    fn put(person: &str, key: &str, status: &str, reason: &str, at: u64) -> Value {
        json!({
            "op": "service_record",
            "person": person,
            "key": key,
            "service": "google_calendar",
            "action": "create_event",
            "status": status,
            "reason": reason,
            "detail": {"title": "Hamburger party", "start": "2026-11-15T19:00:00+07:00", "end": "2026-11-15T22:00:00+07:00"},
            "asked_unix_ms": NOW,
            "changed_unix_ms": at,
        })
    }

    fn records(store: &mut Days, person: &str) -> Vec<Value> {
        let listed = send(store, json!({"op":"service_records","person":person}));
        assert_eq!(listed["ok"], true, "{listed}");
        listed["records"].as_array().unwrap().clone()
    }

    /// A record is created `prepared`, updated in place to a final status
    /// (one row, never one a step), and a final status is never changed by
    /// a later put: the status kept is answered. Each status the plan
    /// names is accepted; anything else is refused.
    #[test]
    fn a_record_is_one_row_updated_in_place_and_a_final_status_stays() {
        let (_scratch, mut store) = Scratch::new("in-place");
        assert_eq!(
            send(&mut store, put(OWNER, "r1", "prepared", "", NOW)),
            json!({"ok":true,"status":"prepared"})
        );
        assert_eq!(
            send(
                &mut store,
                put(OWNER, "r1", "cancelled", "no send heard", NOW + 1_000)
            ),
            json!({"ok":true,"status":"cancelled"})
        );
        let kept = records(&mut store, OWNER);
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert_eq!(kept[0]["status"], "cancelled");
        assert_eq!(kept[0]["reason"], "no send heard");
        assert_eq!(kept[0]["detail"]["title"], "Hamburger party");
        assert_eq!(kept[0]["changed_unix_ms"], NOW + 1_000);
        // Final: a late "done" or "prepared" for it changes nothing.
        for status in ["done", "prepared"] {
            assert_eq!(
                send(&mut store, put(OWNER, "r1", status, "", NOW + 2_000)),
                json!({"ok":true,"status":"cancelled"})
            );
        }
        assert_eq!(records(&mut store, OWNER)[0]["status"], "cancelled");
        // Every status of the plan, each through prepared.
        for (n, status) in super::STATUSES.iter().enumerate() {
            let key = format!("s{n}");
            send(&mut store, put(OWNER, &key, "prepared", "", NOW));
            let set = send(&mut store, put(OWNER, &key, status, "why", NOW + 5_000));
            assert_eq!(set["status"], *status, "{set}");
        }
        // A redo: the new one links back, and the old final one says who
        // replaced it; its status stays.
        let mut redo = put(OWNER, "r2", "prepared", "", NOW + 3_000);
        redo["links"] = json!(["r1"]);
        redo["result_id"] = json!("evt123");
        assert_eq!(send(&mut store, redo)["status"], "prepared");
        let mut replaced = put(OWNER, "r1", "cancelled", "no send heard", NOW + 3_000);
        replaced["replaced_by"] = json!("r2");
        assert_eq!(send(&mut store, replaced)["status"], "cancelled");
        let listed = records(&mut store, OWNER);
        let r1 = listed.iter().find(|r| r["key"] == "r1").unwrap();
        let r2 = listed.iter().find(|r| r["key"] == "r2").unwrap();
        assert_eq!(r1["replaced_by"], "r2");
        assert_eq!(r1["status"], "cancelled");
        assert_eq!(r2["links"], json!(["r1"]));
        assert_eq!(r2["result_id"], "evt123");
        let refused = send(&mut store, put(OWNER, "x", "maybe", "", NOW));
        assert_eq!(refused["error"], "bad_status", "{refused}");
    }

    /// Only a known person has records: never the room's, a rules scope's,
    /// nor with a bad key or detail.
    #[test]
    fn records_are_a_known_persons_only() {
        let (_scratch, mut store) = Scratch::new("known-only");
        let mut room = put(OWNER, "r1", "done", "", NOW);
        room.as_object_mut().unwrap().remove("person");
        assert_eq!(send(&mut store, room)["error"], "bad_person");
        assert_eq!(
            send(&mut store, put("r-0123456789abcdef", "r1", "done", "", NOW))["error"],
            "bad_person"
        );
        assert_eq!(
            send(&mut store, put(OWNER, "a b", "done", "", NOW))["error"],
            "bad_key"
        );
        let mut text = put(OWNER, "r2", "done", "", NOW);
        text["detail"] = json!("not an object");
        assert_eq!(send(&mut store, text)["error"], "bad_detail");
        let mut long = put(OWNER, "r3", "done", "", NOW);
        long["detail"] = json!({"text": "x".repeat(super::DETAIL_BYTES)});
        assert_eq!(send(&mut store, long)["error"], "too_long");
        assert!(records(&mut store, OWNER).is_empty());
    }

    /// Kept until forgotten: across a reopen of the store, past the days
    /// kept and the unused-entry expiry, a nightly pass and a tidy, the
    /// record is still there, whole.
    #[test]
    fn a_record_survives_a_restart_expiry_the_night_and_the_tidy() {
        let (scratch, mut store) = Scratch::new("kept");
        send(
            &mut store,
            put(OWNER, "party", "cancelled", "no send heard", NOW),
        );
        let drive = json!({
            "op":"service_record","person":OWNER,"key":"drive","service":"google_drive",
            "action":"search","status":"no_result","detail":{"query":"Sunday"},
            "asked_unix_ms":NOW,"changed_unix_ms":NOW,
        });
        assert_eq!(send(&mut store, drive)["status"], "no_result");
        // An exchange of the same day, so the night and the tidy have work.
        let said = send(
            &mut store,
            json!({"op":"append","person":OWNER,"at_unix_ms":NOW,"heard":"make a party event","said":"Okay."}),
        );
        assert_eq!(said["ok"], true, "{said}");
        drop(store);
        let mut store = scratch.open();
        assert_eq!(records(&mut store, OWNER).len(), 2);
        // The tidy of the conversation, with nothing proposed.
        let ended = send(
            &mut store,
            json!({"op":"conversation_end","person":OWNER,"conversation":"c1","source_ids":[said["id"]]}),
        );
        assert_eq!(ended["ok"], true, "{ended}");
        if let Some(job) = ended["job"].as_str() {
            let applied = send(
                &mut store,
                json!({"op":"tidy_apply","person":OWNER,"job":job,"observations":[]}),
            );
            assert_eq!(applied["ok"], true, "{applied}");
        }
        // The night, with nothing proposed.
        let night = NOW + DAY_MS;
        let prepared = send_at(
            &mut store,
            json!({"op":"nightly_prepare","person":OWNER,"day":"2026-10-14"}),
            night,
        );
        if let Some(job) = prepared["input"]["job"].as_str() {
            let applied = send_at(
                &mut store,
                json!({"op":"nightly_apply","person":OWNER,"job":job,"proposal":{}}),
                night,
            );
            assert_eq!(applied["ok"], true, "{applied}");
        }
        // A year on: past the 30 days kept and every unused-entry expiry.
        let later = NOW + 400 * DAY_MS;
        store.maintain(later).unwrap();
        let status = send_at(&mut store, json!({"op":"status","person":OWNER}), later);
        assert_eq!(status["ok"], true, "{status}");
        drop(store);
        let mut store = scratch.open();
        let kept = records(&mut store, OWNER);
        assert_eq!(kept.len(), 2, "{kept:?}");
        let party = kept.iter().find(|r| r["key"] == "party").unwrap();
        assert_eq!(party["status"], "cancelled");
        assert_eq!(party["reason"], "no send heard");
        assert_eq!(party["detail"]["start"], "2026-11-15T19:00:00+07:00");
        let drive = kept.iter().find(|r| r["key"] == "drive").unwrap();
        assert_eq!(drive["status"], "no_result");
        assert_eq!(drive["detail"]["query"], "Sunday");
    }

    /// Forget everything takes the person's records (and counts them), and
    /// no one else's; forgetting every person takes all; forgetting the
    /// person takes theirs.
    #[test]
    fn forget_everything_and_forget_the_person_remove_records() {
        let (_scratch, mut store) = Scratch::new("forget-all");
        send(&mut store, put(OWNER, "a", "done", "", NOW));
        send(&mut store, put(OTHER, "b", "done", "", NOW));
        let forgot = send(&mut store, json!({"op":"forget_all","person":OWNER}));
        assert_eq!(forgot["ok"], true, "{forgot}");
        assert!(forgot["removed"].as_u64().unwrap_or(0) >= 1, "{forgot}");
        assert!(records(&mut store, OWNER).is_empty());
        assert_eq!(records(&mut store, OTHER).len(), 1);
        let gone = send(&mut store, json!({"op":"forget_person","person":OTHER}));
        assert_eq!(gone["ok"], true, "{gone}");
        assert!(records(&mut store, OTHER).is_empty());
        send(&mut store, put(OWNER, "c", "done", "", NOW));
        send(&mut store, put(OTHER, "d", "done", "", NOW));
        let every = send(&mut store, json!({"op":"forget_all","person":null}));
        assert_eq!(every["ok"], true, "{every}");
        assert!(records(&mut store, OWNER).is_empty());
        assert!(records(&mut store, OTHER).is_empty());
    }

    /// `service_records_forget` with `all` takes every record of that
    /// person and no one else's, and refuses a put asked before it: the
    /// voice's forget everything for the services' owner id in the private
    /// room (the switch-on row on lamp-06c1, 11 Nov).
    #[test]
    fn a_records_forget_with_all_takes_every_record_of_that_person() {
        let (_scratch, mut store) = Scratch::new("forget-records-all");
        send(&mut store, put(OWNER, "a", "done", "", NOW));
        send(&mut store, put(OWNER, "b", "cancelled", "", NOW));
        send(&mut store, put(OTHER, "c", "done", "", NOW));
        let forgot = send_at(
            &mut store,
            json!({"op":"service_records_forget","person":OWNER,"all":true}),
            NOW + 1_000,
        );
        assert_eq!(forgot, json!({"ok":true,"removed":2}));
        assert!(records(&mut store, OWNER).is_empty());
        assert_eq!(records(&mut store, OTHER).len(), 1);
        let late = send_at(&mut store, put(OWNER, "a", "done", "", NOW), NOW + 1_500);
        assert_eq!(late["error"], "forgotten", "{late}");
    }

    /// A forget about a thing takes the records that have each of its key
    /// words (lower case, without accents, a plural as its singular), and
    /// no other; another person's never.
    #[test]
    fn forget_about_takes_only_the_matching_records() {
        let (_scratch, mut store) = Scratch::new("forget-about");
        send(&mut store, put(OWNER, "party", "cancelled", "", NOW));
        send(&mut store, put(OTHER, "party", "cancelled", "", NOW));
        let mail = json!({
            "op":"service_record","person":OWNER,"key":"mail","service":"gmail",
            "action":"send_mail","status":"done",
            "detail":{"to":["octavia@example.com"],"subject":"Đi chơi Chủ nhật","text":"See you"},
            "asked_unix_ms":NOW,"changed_unix_ms":NOW,
        });
        send(&mut store, mail);
        let forget = |store: &mut Days, about: Value| -> Value {
            send(
                store,
                json!({"op":"service_records_forget","person":OWNER,"about":about}),
            )
        };
        assert_eq!(forget(&mut store, json!([["birthday"]]))["removed"], 0);
        assert_eq!(
            forget(&mut store, json!([["hamburger", "dinner"]]))["removed"],
            0
        );
        assert_eq!(forget(&mut store, json!([["parties"]]))["removed"], 1);
        let left = records(&mut store, OWNER);
        assert_eq!(left.len(), 1, "{left:?}");
        assert_eq!(left[0]["key"], "mail");
        assert_eq!(forget(&mut store, json!([["di", "choi"]]))["removed"], 1);
        assert!(records(&mut store, OWNER).is_empty());
        assert_eq!(records(&mut store, OTHER).len(), 1);
    }

    fn asked(mut put: Value, asked: u64) -> Value {
        put["asked_unix_ms"] = json!(asked);
        put["changed_unix_ms"] = json!(asked);
        put
    }

    /// A record asked before the latest forget of its person's everything
    /// (forget everything, forgetting the person, everyone's) is refused,
    /// as one sent before the forget and taken after it (Jarvis's P2 on
    /// #6073), even after a restart; one asked after it is kept, and
    /// another person's is untouched.
    #[test]
    fn a_record_asked_before_a_forget_everything_is_refused() {
        let (scratch, mut store) = Scratch::new("forget-mark");
        send(&mut store, put(OWNER, "r1", "prepared", "", NOW));
        let forgot = send_at(
            &mut store,
            json!({"op":"forget_all","person":OWNER}),
            NOW + 1_000,
        );
        assert_eq!(forgot["ok"], true, "{forgot}");
        // In flight at the forget: refused, nothing kept.
        let late = send_at(&mut store, put(OWNER, "r1", "done", "", NOW), NOW + 1_500);
        assert_eq!(late["ok"], false, "{late}");
        assert_eq!(late["error"], "forgotten", "{late}");
        assert!(records(&mut store, OWNER).is_empty());
        // Asked after it: kept.
        let after = send_at(
            &mut store,
            asked(put(OWNER, "r2", "done", "", NOW), NOW + 2_000),
            NOW + 2_000,
        );
        assert_eq!(after, json!({"ok":true,"status":"done"}));
        // Another person's, asked at the same time: theirs, kept.
        let other = send_at(&mut store, put(OTHER, "o1", "done", "", NOW), NOW + 2_000);
        assert_eq!(other["ok"], true, "{other}");
        // Forgetting the person marks them too.
        send_at(
            &mut store,
            json!({"op":"forget_person","person":OTHER}),
            NOW + 3_000,
        );
        let other = send_at(
            &mut store,
            asked(put(OTHER, "o2", "done", "", NOW), NOW + 2_500),
            NOW + 3_000,
        );
        assert_eq!(other["error"], "forgotten", "{other}");
        // Everyone's, and still marked after a restart.
        send_at(
            &mut store,
            json!({"op":"forget_all","person":null}),
            NOW + 4_000,
        );
        drop(store);
        let mut store = scratch.open();
        let late = send_at(
            &mut store,
            asked(put(OWNER, "r3", "done", "", NOW), NOW + 3_500),
            NOW + 5_000,
        );
        assert_eq!(late["error"], "forgotten", "{late}");
        let after = send_at(
            &mut store,
            asked(put(OWNER, "r4", "done", "", NOW), NOW + 4_000),
            NOW + 5_000,
        );
        assert_eq!(after["ok"], true, "{after}");
        let kept = records(&mut store, OWNER);
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert_eq!(kept[0]["key"], "r4");
    }

    /// A record asked before a forget about a thing that matches it is
    /// refused (Jarvis's P2 on #6073), found by the forget or not; one
    /// that does not match, or asked after it, is kept.
    #[test]
    fn a_record_asked_before_a_forget_about_it_is_refused() {
        let (_scratch, mut store) = Scratch::new("forget-about-mark");
        let forgot = send_at(
            &mut store,
            json!({"op":"service_records_forget","person":OWNER,"about":[["parties"]]}),
            NOW + 1_000,
        );
        assert_eq!(forgot, json!({"ok":true,"removed":0}));
        let late = send_at(&mut store, put(OWNER, "p1", "done", "", NOW), NOW + 1_500);
        assert_eq!(late["error"], "forgotten", "{late}");
        let drive = json!({
            "op":"service_record","person":OWNER,"key":"d1","service":"google_drive",
            "action":"files","status":"no_result","detail":{"query":"Sunday"},
            "asked_unix_ms":NOW,"changed_unix_ms":NOW,
        });
        assert_eq!(send_at(&mut store, drive, NOW + 1_500)["ok"], true);
        // Another person's party is theirs.
        let other = send_at(&mut store, put(OTHER, "p1", "done", "", NOW), NOW + 1_500);
        assert_eq!(other["ok"], true, "{other}");
        let after = send_at(
            &mut store,
            asked(put(OWNER, "p2", "done", "", NOW), NOW + 2_000),
            NOW + 2_000,
        );
        assert_eq!(after["ok"], true, "{after}");
        let mut kept: Vec<String> = records(&mut store, OWNER)
            .iter()
            .map(|record| record["key"].as_str().unwrap().to_owned())
            .collect();
        kept.sort();
        assert_eq!(kept, ["d1", "p2"]);
    }

    /// `LAMP_SERVICE_RECORDS_TOLD`: a recall's words find the record they
    /// are about, whatever its age (past the 50 latest a list gives), with
    /// all its details; the best match first; small words find nothing;
    /// another person's never.
    #[test]
    fn a_recall_finds_a_record_by_its_words_whatever_its_age() {
        let (_scratch, mut store) = Scratch::new("about");
        let year_ago = NOW - 365 * DAY_MS;
        let mut party = put(
            OWNER,
            "party",
            "cancelled",
            "the lamp answered after the read-back with no send",
            year_ago,
        );
        party["asked_unix_ms"] = json!(year_ago);
        send(&mut store, party);
        let mail = json!({
            "op":"service_record","person":OWNER,"key":"mail","service":"gmail",
            "action":"send_mail","status":"done","result_id":"m-77",
            "detail":{"to":["octaviab@example.com"],"subject":"Sunday","text":"Hi Octavia, see you Sunday"},
            "asked_unix_ms":year_ago + 1,"changed_unix_ms":year_ago + 1,
        });
        send(&mut store, mail);
        send(&mut store, put(OTHER, "theirs", "done", "", NOW));
        // Sixty newer asks, so neither is among the latest a list gives.
        for n in 0..60u64 {
            let drive = json!({
                "op":"service_record","person":OWNER,"key":format!("d{n}"),"service":"google_drive",
                "action":"search","status":"no_result","detail":{"query":"invoice"},
                "asked_unix_ms":NOW + n,"changed_unix_ms":NOW + n,
            });
            send(&mut store, drive);
        }
        let latest = records(&mut store, OWNER);
        assert!(
            latest
                .iter()
                .all(|r| r["key"] != "party" && r["key"] != "mail")
        );
        let about = |store: &mut Days, words: &str| -> Vec<Value> {
            let found = send(
                store,
                json!({"op":"service_records","person":OWNER,"about":words,"limit":5}),
            );
            assert_eq!(found["ok"], true, "{found}");
            found["records"].as_array().unwrap().clone()
        };
        let found = about(&mut store, "that party event");
        assert_eq!(
            found.first().map(|r| r["key"].clone()),
            Some(json!("party")),
            "{found:?}"
        );
        assert_eq!(found[0]["status"], "cancelled");
        assert_eq!(
            found[0]["reason"],
            "the lamp answered after the read-back with no send"
        );
        assert_eq!(found[0]["detail"]["start"], "2026-11-15T19:00:00+07:00");
        let found = about(&mut store, "the email to Octavia");
        assert_eq!(
            found.first().map(|r| r["key"].clone()),
            Some(json!("mail")),
            "{found:?}"
        );
        assert_eq!(found[0]["detail"]["text"], "Hi Octavia, see you Sunday");
        assert_eq!(found[0]["result_id"], "m-77");
        assert_eq!(about(&mut store, "the parties").len(), 1);
        assert!(about(&mut store, "birthday").is_empty());
        assert!(about(&mut store, "did you do that").is_empty());
        assert!(about(&mut store, "invoice").len() <= 5);
    }

    /// Bounded: past the most a person keeps, the record changed longest
    /// ago goes; the list gives the latest change first.
    #[test]
    fn records_are_bounded_and_listed_latest_first() {
        let (_scratch, mut store) = Scratch::new("bounded");
        let person = crate::store::Person::new(OWNER).unwrap();
        for n in 0..3u64 {
            let record = super::RecordIn {
                key: format!("k{n}"),
                service: "gmail".into(),
                action: "send_mail".into(),
                status: "done".into(),
                reason: String::new(),
                detail: None,
                result_id: String::new(),
                asked_unix_ms: NOW + n,
                changed_unix_ms: NOW + n,
                links: vec![],
                replaced_by: String::new(),
                source_kind: None,
                source_ref: None,
                summary: None,
                deed_unix_ms: None,
            };
            store.service_record_put(&person, &record).unwrap();
        }
        let listed = store.service_records(&person, 2).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0]["key"], "k2");
        assert_eq!(listed[1]["key"], "k1");
    }

    /// A linked put: `kind` with a source and a summary, done at `deed`.
    fn linked(person: &str, key: &str, kind: &str, summary: &str, deed: u64) -> Value {
        json!({
            "op": "service_record",
            "person": person,
            "key": key,
            "service": "lamp_tasks",
            "action": "task_settled",
            "status": "done",
            "asked_unix_ms": deed,
            "changed_unix_ms": deed,
            "source_kind": kind,
            "source_ref": {"service": "lamp-tasks", "id": format!("{key}-id"), "computer": "c-1"},
            "summary": summary,
            "deed_unix_ms": deed,
        })
    }

    fn database(scratch: &Scratch) -> rusqlite::Connection {
        rusqlite::Connection::open(scratch.0.join(crate::days::DATABASE)).unwrap()
    }

    fn columns(db: &rusqlite::Connection) -> Vec<String> {
        let mut statement = db
            .prepare("SELECT name FROM pragma_table_info('service_records')")
            .unwrap();
        statement
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<String>>>()
            .unwrap()
    }

    /// The linked columns are added once, by `pragma table_info`, however
    /// often the store opens; their index is made; `user_version` stays 1.
    #[test]
    fn the_linked_columns_migration_is_idempotent() {
        let (scratch, store) = Scratch::new("migrate-twice");
        drop(store);
        for _ in 0..2 {
            drop(scratch.open());
        }
        let db = database(&scratch);
        let have = columns(&db);
        for column in ["source_kind", "source_ref", "summary", "deed_unix_ms"] {
            assert_eq!(
                have.iter().filter(|name| *name == column).count(),
                1,
                "{have:?}"
            );
        }
        let index: u64 = db
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='index' AND name='service_records_kind'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(index, 1);
        let version: u64 = db
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 1);
    }

    /// A database the current `lamp-voice` schema made (the table without
    /// the linked columns, one record in it) opens: its record is a
    /// `voice` one with no summary, pointer or deed, and an old-style put
    /// still works. An older build's insert into the migrated table (its
    /// own column list, after its `days.sql` runs again) still works too:
    /// the rollback case.
    #[test]
    fn an_older_database_opens_and_works_after_the_migration() {
        let (scratch, store) = Scratch::new("older-db");
        drop(store);
        fs::remove_dir_all(&scratch.0).unwrap();
        crate::store::private_dir(&scratch.0).unwrap();
        {
            let db = database(&scratch);
            db.execute_batch(include_str!("days.sql")).unwrap();
            assert!(!columns(&db).iter().any(|name| name == "source_kind"));
            db.execute_batch(&format!(
                "INSERT INTO settings(key,value) VALUES('import_complete',1); PRAGMA user_version=1;
                 INSERT INTO people(person) VALUES('{OWNER}');
                 INSERT INTO service_records(person,key,service,action,status,reason,detail,result_id,asked_unix_ms,changed_unix_ms,links,replaced_by)
                 VALUES('{OWNER}','old','gmail','send_mail','done','','{{\"subject\":\"Sunday\"}}','',{NOW},{NOW},'[]','');"
            ))
            .unwrap();
        }
        // Owner-only, as the store made it.
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            scratch.0.join(crate::days::DATABASE),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        let mut store = scratch.open();
        let kept = records(&mut store, OWNER);
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert_eq!(kept[0]["key"], "old");
        assert_eq!(kept[0]["source_kind"], "voice");
        assert_eq!(kept[0]["source_ref"], json!({}));
        assert_eq!(kept[0]["summary"], "");
        assert_eq!(kept[0]["deed_unix_ms"], Value::Null);
        assert_eq!(kept[0]["detail"]["subject"], "Sunday");
        assert_eq!(
            send(&mut store, put(OWNER, "new", "done", "", NOW + 1)),
            json!({"ok":true,"status":"done"})
        );
        drop(store);
        {
            let db = database(&scratch);
            db.execute_batch(include_str!("days.sql")).unwrap();
            db.execute(
                "INSERT INTO service_records(person,key,service,action,status,reason,detail,result_id,asked_unix_ms,changed_unix_ms,links,replaced_by) VALUES(?1,'rolled','gmail','send_mail','done','','{}','',?2,?2,'[]','')",
                rusqlite::params![OWNER, NOW as i64 + 2],
            )
            .unwrap();
        }
        let mut store = scratch.open();
        let kept = records(&mut store, OWNER);
        assert_eq!(kept.len(), 3, "{kept:?}");
        assert!(kept.iter().all(|r| r["source_kind"] == "voice"), "{kept:?}");
    }

    /// A put that names none of the linked fields (an old voice's) is a
    /// `voice` record; an update of it without them keeps what is held.
    #[test]
    fn an_old_style_put_is_a_voice_record() {
        let (_scratch, mut store) = Scratch::new("old-style");
        assert_eq!(
            send(&mut store, put(OWNER, "r1", "prepared", "", NOW))["status"],
            "prepared"
        );
        let mut summed = put(OWNER, "r1", "prepared", "", NOW + 1);
        summed["summary"] = json!("Hamburger party on Sunday");
        send(&mut store, summed);
        send(&mut store, put(OWNER, "r1", "done", "", NOW + 2));
        let kept = records(&mut store, OWNER);
        assert_eq!(kept[0]["status"], "done");
        assert_eq!(kept[0]["source_kind"], "voice");
        assert_eq!(kept[0]["source_ref"], json!({}));
        assert_eq!(kept[0]["summary"], "Hamburger party on Sunday");
        // A record keeps its kind.
        let mut task = linked(OWNER, "r1", "task", "", NOW + 3);
        task["status"] = json!("done");
        assert_eq!(send(&mut store, task)["error"], "bad_kind");
    }

    /// Past [`super::MOST_PER_KIND`] of a linked kind, that kind's record
    /// changed longest ago goes: no other kind's, and no voice record.
    #[test]
    fn the_per_kind_cap_trims_only_that_kind() {
        let (scratch, store) = Scratch::new("per-kind");
        drop(store);
        {
            let mut db = database(&scratch);
            let tx = db.transaction().unwrap();
            tx.execute("INSERT INTO people(person) VALUES(?1)", [OWNER])
                .unwrap();
            let add = |key: String, kind: &str, at: u64| {
                tx.execute(
                    "INSERT INTO service_records(person,key,service,action,status,asked_unix_ms,changed_unix_ms,source_kind,source_ref) VALUES(?1,?2,'s','a','done',?3,?3,?4,'{\"service\":\"s\",\"id\":\"i\"}')",
                    rusqlite::params![OWNER, key, at as i64, kind],
                )
                .unwrap();
            };
            // The voice and agent records are older than every task one.
            for n in 0..5u64 {
                add(format!("v{n}"), "voice", NOW - 1_000 + n);
                add(format!("a{n}"), "agent", NOW - 900 + n);
            }
            for n in 0..super::MOST_PER_KIND as u64 {
                add(format!("t{n}"), "task", NOW + n);
            }
            tx.commit().unwrap();
        }
        let mut store = scratch.open();
        let late = NOW + 10_000;
        assert_eq!(
            send_at(&mut store, linked(OWNER, "tnew", "task", "", late), late)["ok"],
            true
        );
        drop(store);
        let db = database(&scratch);
        let count = |kind: &str| -> usize {
            db.query_row(
                "SELECT count(*) FROM service_records WHERE person=?1 AND source_kind=?2",
                [OWNER, kind],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(count("task"), super::MOST_PER_KIND);
        assert_eq!(count("agent"), 5);
        assert_eq!(count("voice"), 5);
        let gone: usize = db
            .query_row(
                "SELECT count(*) FROM service_records WHERE key='t0'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(gone, 0);
    }

    /// Forget everything removes the linked records with the voice ones,
    /// pointer and summary included.
    #[test]
    fn forget_all_removes_linked_records() {
        let (scratch, mut store) = Scratch::new("forget-linked");
        send(
            &mut store,
            linked(OWNER, "t1", "task", "Booked the dentist", NOW),
        );
        send(
            &mut store,
            linked(OWNER, "m1", "reminder", "Take the pills", NOW),
        );
        send(
            &mut store,
            linked(OWNER, "a1", "agent", "Read the inbox", NOW),
        );
        send(&mut store, put(OWNER, "v1", "done", "", NOW));
        assert_eq!(records(&mut store, OWNER).len(), 4);
        let forgot = send_at(
            &mut store,
            json!({"op":"forget_all","person":OWNER}),
            NOW + 1_000,
        );
        assert_eq!(forgot["ok"], true, "{forgot}");
        assert!(records(&mut store, OWNER).is_empty());
        drop(store);
        let left: u64 = database(&scratch)
            .query_row("SELECT count(*) FROM service_records", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 0);
    }

    /// A forget about a thing matches a record's summary: it takes only
    /// the record whose summary names it; a recall finds it by it too.
    #[test]
    fn a_forget_about_a_summary_takes_only_that_record() {
        let (_scratch, mut store) = Scratch::new("forget-summary");
        send(
            &mut store,
            linked(OWNER, "t1", "task", "Booked the dentist for Friday", NOW),
        );
        send(
            &mut store,
            linked(OWNER, "t2", "task", "Ordered printer paper", NOW),
        );
        let found = send(
            &mut store,
            json!({"op":"service_records","person":OWNER,"about":"the dentist","limit":5}),
        );
        assert_eq!(found["records"].as_array().unwrap().len(), 1, "{found}");
        assert_eq!(found["records"][0]["key"], "t1");
        let forgot = send_at(
            &mut store,
            json!({"op":"service_records_forget","person":OWNER,"about":[["dentist"]]}),
            NOW + 1_000,
        );
        assert_eq!(forgot, json!({"ok":true,"removed":1}));
        let left = records(&mut store, OWNER);
        assert_eq!(left.len(), 1, "{left:?}");
        assert_eq!(left[0]["key"], "t2");
        assert_eq!(left[0]["summary"], "Ordered printer paper");
    }

    /// A forget wins by the deed's time: a put whose deed is before the
    /// person's last forget is refused, even asked after it; a deed after
    /// it is kept. A forget about a thing refuses a matching earlier deed.
    #[test]
    fn a_put_with_a_deed_before_the_last_forget_is_refused() {
        let (_scratch, mut store) = Scratch::new("deed-forget");
        send_at(
            &mut store,
            json!({"op":"forget_all","person":OWNER}),
            NOW + 1_000,
        );
        let mut early = linked(OWNER, "t1", "task", "Booked the dentist", NOW + 500);
        early["asked_unix_ms"] = json!(NOW + 2_000);
        early["changed_unix_ms"] = json!(NOW + 2_000);
        let late = send_at(&mut store, early, NOW + 2_000);
        assert_eq!(late["error"], "forgotten", "{late}");
        let after = send_at(
            &mut store,
            linked(OWNER, "t2", "task", "Booked the dentist", NOW + 1_500),
            NOW + 2_000,
        );
        assert_eq!(after["ok"], true, "{after}");
        send_at(
            &mut store,
            json!({"op":"service_records_forget","person":OWNER,"about":[["dentist"]]}),
            NOW + 3_000,
        );
        let mut early = linked(OWNER, "t3", "task", "Called the dentist", NOW + 2_500);
        early["asked_unix_ms"] = json!(NOW + 4_000);
        early["changed_unix_ms"] = json!(NOW + 4_000);
        assert_eq!(
            send_at(&mut store, early, NOW + 4_000)["error"],
            "forgotten"
        );
        let other = linked(OWNER, "t4", "task", "Ordered paper", NOW + 2_500);
        assert_eq!(send_at(&mut store, other, NOW + 4_000)["ok"], true);
        let mut kept: Vec<String> = records(&mut store, OWNER)
            .iter()
            .map(|r| r["key"].as_str().unwrap().to_owned())
            .collect();
        kept.sort();
        assert_eq!(kept, ["t4"]);
    }

    /// The summary is cut at a whole character to its 160 bytes and its
    /// control characters stripped; a `source_ref` past 512 bytes, not an
    /// object, without its service or id, with another field, or missing
    /// from a linked kind is refused, as is an unknown kind.
    #[test]
    fn the_linked_bounds_hold() {
        let (_scratch, mut store) = Scratch::new("linked-bounds");
        let long = format!("Đặt\u{7} lịch\n{}", "é".repeat(200));
        assert_eq!(
            send(&mut store, linked(OWNER, "t1", "task", &long, NOW))["ok"],
            true
        );
        let summary = records(&mut store, OWNER)[0]["summary"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(summary.len() <= super::SUMMARY_BYTES, "{}", summary.len());
        assert!(
            summary.len() > super::SUMMARY_BYTES - 2,
            "{}",
            summary.len()
        );
        assert!(summary.starts_with("Đặt lịch é"), "{summary}");
        assert!(!summary.chars().any(char::is_control), "{summary}");
        let refused = |store: &mut Days, source_ref: Value| -> Value {
            let mut put = linked(OWNER, "t2", "reminder", "", NOW);
            put["source_ref"] = source_ref;
            send(store, put)
        };
        for source_ref in [
            json!({"service":"lamp-schedule","id":"x".repeat(520)}),
            json!("{\"service\":\"lamp-schedule\",\"id\":\"r1\"}"),
            json!({"service":"lamp-schedule"}),
            json!({"id":"r1"}),
            json!({"service":"lamp-schedule","id":"r1","text":"secret"}),
            json!({"service":"lamp-schedule","id":7}),
            json!({"service":"lamp-schedule","id":"r\u{1}1"}),
            Value::Null,
        ] {
            let answer = refused(&mut store, source_ref.clone());
            assert_eq!(answer["error"], "bad_source_ref", "{source_ref} {answer}");
        }
        let fits = json!({"service":"lamp-schedule","id":"r1","session":"s-9","computer":"c-1"});
        assert_eq!(refused(&mut store, fits)["ok"], true);
        let mut odd = linked(OWNER, "t3", "mail", "", NOW);
        odd["source_kind"] = json!("mail");
        assert_eq!(send(&mut store, odd)["error"], "bad_kind");
        let kept = records(&mut store, OWNER);
        assert_eq!(kept.len(), 2, "{kept:?}");
        let reminder = kept.iter().find(|r| r["key"] == "t2").unwrap();
        assert_eq!(reminder["source_kind"], "reminder");
        assert_eq!(reminder["source_ref"]["computer"], "c-1");
        assert_eq!(reminder["deed_unix_ms"], NOW);
    }

    /// The task row the voice writes for a known person's task, run as
    /// `run` (slice 1b's owner check matches its `session`).
    fn task_row(person: &str, key: &str, run: &str, deed: u64) -> Value {
        let mut row = linked(person, key, "task", "Book the dentist", deed);
        row["source_ref"] = json!({"service":"lamp-tasks","id":"7","session":run});
        row
    }

    /// What lamp-services sends on `linked.sock` (slice 1b): an agent's
    /// read in `run` of the task `task`, done at `deed`, with no person.
    fn agent_read(task: &str, run: &str, n: u64, deed: u64) -> Value {
        json!({
            "op": "linked_record",
            "source_kind": "agent",
            "task": task,
            "run": run,
            "n": n,
            "action": "search_mail",
            "status": "done",
            "deed_unix_ms": deed,
            "summary": "Searched mail for the dentist",
            "source_ref": {"service": "gmail", "id": "res-1", "session": run},
        })
    }

    /// Slice 1b, condition 1: memory finds the owner itself. An agent's
    /// record is kept under the person whose task row names its run, keyed
    /// `a-<run>-<n>` and linked to that task row; the request names no
    /// person and may not.
    #[test]
    fn a_linked_record_is_kept_for_the_owner_of_its_task_row() {
        let (_scratch, mut store) = Scratch::new("linked-owner");
        assert_eq!(
            send(&mut store, task_row(OWNER, "t-tasks-7", "run-1", NOW))["ok"],
            true
        );
        let kept = send_at(
            &mut store,
            agent_read("t-tasks-7", "run-1", 0, NOW + 10),
            NOW + 20,
        );
        assert_eq!(
            kept,
            json!({"ok":true,"status":"done","key":"a-run-1-0"}),
            "{kept}"
        );
        let rows = records(&mut store, OWNER);
        let agent = rows.iter().find(|r| r["key"] == "a-run-1-0").unwrap();
        assert_eq!(agent["source_kind"], "agent");
        assert_eq!(agent["service"], "gmail");
        assert_eq!(agent["action"], "search_mail");
        assert_eq!(agent["links"], json!(["t-tasks-7"]));
        assert_eq!(agent["summary"], "Searched mail for the dentist");
        assert_eq!(agent["deed_unix_ms"], NOW + 10);
        assert!(records(&mut store, OTHER).is_empty());
        let mut named = agent_read("t-tasks-7", "run-1", 1, NOW + 10);
        named["person"] = json!(OTHER);
        assert_eq!(send(&mut store, named)["error"], "bad_request");
    }

    /// No task row, a task row of another run, a row of the same key that
    /// is not a task, or a task key two people hold: no owner, nothing kept.
    #[test]
    fn a_linked_record_with_no_matching_task_row_is_refused() {
        let (_scratch, mut store) = Scratch::new("linked-no-owner");
        let no_owner = |store: &mut Days, request: Value| {
            let answer = send_at(store, request, NOW + 20);
            assert_eq!(answer["error"], "no_owner", "{answer}");
        };
        no_owner(&mut store, agent_read("t-tasks-7", "run-1", 0, NOW));
        send(&mut store, task_row(OWNER, "t-tasks-7", "run-1", NOW));
        no_owner(&mut store, agent_read("t-tasks-7", "run-2", 0, NOW));
        no_owner(&mut store, agent_read("t-tasks-8", "run-1", 0, NOW));
        send(&mut store, put(OWNER, "t-tasks-9", "done", "", NOW));
        no_owner(&mut store, agent_read("t-tasks-9", "run-1", 0, NOW));
        send(&mut store, task_row(OTHER, "t-tasks-7", "run-1", NOW));
        no_owner(&mut store, agent_read("t-tasks-7", "run-1", 0, NOW));
        let keys: Vec<Value> = records(&mut store, OWNER)
            .iter()
            .map(|r| r["key"].clone())
            .collect();
        assert!(
            !keys
                .iter()
                .any(|key| key.as_str().unwrap().starts_with("a-"))
        );
    }

    /// Condition 4: each request names its source kind, and this op takes
    /// an agent's alone (tasks and reminders are the voice's to write).
    #[test]
    fn a_linked_record_names_its_kind_and_it_is_an_agents() {
        let (_scratch, mut store) = Scratch::new("linked-kind");
        send(&mut store, task_row(OWNER, "t-tasks-7", "run-1", NOW));
        let mut unnamed = agent_read("t-tasks-7", "run-1", 0, NOW);
        unnamed.as_object_mut().unwrap().remove("source_kind");
        assert_eq!(send(&mut store, unnamed)["error"], "bad_request");
        for kind in ["task", "reminder", "sdk", "voice", "mail", ""] {
            let mut other = agent_read("t-tasks-7", "run-1", 0, NOW);
            other["source_kind"] = json!(kind);
            assert_eq!(send(&mut store, other)["error"], "bad_kind", "{kind}");
        }
    }

    /// Condition 2: a forget wins by the deed's time. After a forget of
    /// everything the task row is gone, so nothing has an owner; with the
    /// task row written again after it, a deed from before the forget is
    /// still refused, and a deed more than a few seconds ahead of memory's
    /// clock is refused rather than let pass a later forget.
    #[test]
    fn a_forget_wins_over_a_linked_record_by_its_deed() {
        let (_scratch, mut store) = Scratch::new("linked-forget");
        send(&mut store, task_row(OWNER, "t-tasks-7", "run-1", NOW));
        send_at(
            &mut store,
            json!({"op":"forget_all","person":OWNER}),
            NOW + 1_000,
        );
        let gone = send_at(
            &mut store,
            agent_read("t-tasks-7", "run-1", 0, NOW + 500),
            NOW + 2_000,
        );
        assert_eq!(gone["error"], "no_owner", "{gone}");
        send_at(
            &mut store,
            task_row(OWNER, "t-tasks-7", "run-1", NOW + 1_500),
            NOW + 2_000,
        );
        let early = send_at(
            &mut store,
            agent_read("t-tasks-7", "run-1", 0, NOW + 500),
            NOW + 2_000,
        );
        assert_eq!(early["error"], "forgotten", "{early}");
        let ahead = send_at(
            &mut store,
            agent_read("t-tasks-7", "run-1", 1, NOW + 2_000 + 60_000),
            NOW + 2_000,
        );
        assert_eq!(ahead["error"], "bad_time", "{ahead}");
        let after = send_at(
            &mut store,
            agent_read("t-tasks-7", "run-1", 2, NOW + 1_800),
            NOW + 2_000,
        );
        assert_eq!(after["ok"], true, "{after}");
    }

    /// Condition 5 and the key's shape: the summary is cut to its 160
    /// bytes; a run or a task key that would not make a record key, a task
    /// key not a task's (`t-`), and a `source_ref` past its bounds are
    /// refused.
    #[test]
    fn a_linked_records_bounds_hold() {
        let (_scratch, mut store) = Scratch::new("linked-bounds-1b");
        send(&mut store, task_row(OWNER, "t-tasks-7", "run-1", NOW));
        let mut long = agent_read("t-tasks-7", "run-1", 0, NOW);
        long["summary"] = json!("é".repeat(200));
        assert_eq!(send(&mut store, long)["ok"], true);
        let kept = records(&mut store, OWNER);
        let summary = kept.iter().find(|r| r["key"] == "a-run-1-0").unwrap()["summary"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(summary.len() <= super::SUMMARY_BYTES, "{}", summary.len());
        for (task, run) in [
            ("t-tasks-7", "run 1"),
            ("t-tasks-7", ""),
            ("t-tasks-7", &"r".repeat(61)),
            ("tasks-7", "run-1"),
            ("t-tasks/7", "run-1"),
        ] {
            let answer = send(&mut store, agent_read(task, run, 0, NOW));
            assert_eq!(answer["error"], "bad_key", "{task} {run} {answer}");
        }
        let mut big = agent_read("t-tasks-7", "run-1", 3, NOW);
        big["source_ref"]["id"] = json!("x".repeat(600));
        assert_eq!(send(&mut store, big)["error"], "bad_source_ref");
        let mut serviceless = agent_read("t-tasks-7", "run-1", 4, NOW);
        serviceless["source_ref"]
            .as_object_mut()
            .unwrap()
            .remove("service");
        assert_eq!(send(&mut store, serviceless)["error"], "bad_source_ref");
    }

    /// A task row whose words share none with the agent's read (slice 1b's
    /// P2 on #7463: the task row survives a forget about the read).
    fn plain_task_row(person: &str, run: &str) -> Value {
        let mut row = task_row(person, "t-tasks-7", run, NOW);
        row["summary"] = json!("Plan the week");
        row
    }

    /// A forget about a thing takes an agent's linked record whose summary
    /// names it, and leaves its task row: a recall no longer finds it.
    #[test]
    fn a_forget_about_takes_an_agents_linked_record() {
        let (_scratch, mut store) = Scratch::new("linked-forget-about");
        send(&mut store, plain_task_row(OWNER, "run-1"));
        let mut read = agent_read("t-tasks-7", "run-1", 0, NOW + 10);
        read["summary"] = json!("Booked the dentist");
        assert_eq!(send_at(&mut store, read, NOW + 20)["ok"], true);
        let forgot = send_at(
            &mut store,
            json!({"op":"service_records_forget","person":OWNER,"about":[["dentist"]]}),
            NOW + 1_000,
        );
        assert_eq!(forgot, json!({"ok":true,"removed":1}));
        let left = records(&mut store, OWNER);
        assert_eq!(left.len(), 1, "{left:?}");
        assert_eq!(left[0]["key"], "t-tasks-7");
        let found = send(
            &mut store,
            json!({"op":"service_records","person":OWNER,"about":"the dentist","limit":5}),
        );
        assert!(found["records"].as_array().unwrap().is_empty(), "{found}");
    }

    /// El's P2 on #7463: after a forget about an agent's record, a late
    /// put of the same key asked before the forget never comes back, with
    /// its summary, with an empty one, or with none. `linked.sock` refuses
    /// a blank summary outright; the store refuses an agent's put with no
    /// summary asked before any forget about a thing of its owner.
    #[test]
    fn a_late_summaryless_linked_record_stays_forgotten() {
        let (_scratch, mut store) = Scratch::new("linked-late-blank");
        send(&mut store, plain_task_row(OWNER, "run-1"));
        let mut read = agent_read("t-tasks-7", "run-1", 0, NOW + 10);
        read["summary"] = json!("Booked the dentist");
        assert_eq!(send_at(&mut store, read.clone(), NOW + 20)["ok"], true);
        send_at(
            &mut store,
            json!({"op":"service_records_forget","person":OWNER,"about":[["dentist"]]}),
            NOW + 1_000,
        );
        assert_eq!(
            send_at(&mut store, read.clone(), NOW + 2_000)["error"],
            "forgotten"
        );
        for blank in [json!(""), json!("   ")] {
            let mut late = read.clone();
            late["summary"] = blank;
            let answer = send_at(&mut store, late, NOW + 2_000);
            assert_eq!(answer["error"], "empty", "{answer}");
        }
        let mut unsaid = read.clone();
        unsaid.as_object_mut().unwrap().remove("summary");
        assert_eq!(
            send_at(&mut store, unsaid, NOW + 2_000)["error"],
            "bad_request"
        );
        // The same row put straight to the store, as an agent's, asked
        // before the forget: refused with an empty summary and with none.
        let mut direct = linked(OWNER, "a-run-1-0", "agent", "", NOW + 10);
        assert_eq!(
            send_at(&mut store, direct.clone(), NOW + 2_000)["error"],
            "forgotten"
        );
        direct.as_object_mut().unwrap().remove("summary");
        assert_eq!(
            send_at(&mut store, direct, NOW + 2_000)["error"],
            "forgotten"
        );
        let keys: Vec<Value> = records(&mut store, OWNER)
            .iter()
            .map(|r| r["key"].clone())
            .collect();
        assert_eq!(keys, vec![json!("t-tasks-7")]);
        // Asked after the forget, a read of the run is kept as before.
        let mut fresh = agent_read("t-tasks-7", "run-1", 1, NOW + 1_500);
        fresh["summary"] = json!("Called the dentist back");
        assert_eq!(send_at(&mut store, fresh, NOW + 2_000)["ok"], true);
    }

    /// The app's removal of a person takes their agents' rows with the
    /// rest, and a late read of that run, made before it, is not kept:
    /// no owner while the task row is gone, forgotten once it is back.
    #[test]
    fn forget_person_removes_agent_rows() {
        let (_scratch, mut store) = Scratch::new("linked-forget-person");
        send(&mut store, task_row(OWNER, "t-tasks-7", "run-1", NOW));
        send(&mut store, task_row(OTHER, "t-tasks-8", "run-2", NOW));
        assert_eq!(
            send_at(
                &mut store,
                agent_read("t-tasks-7", "run-1", 0, NOW + 10),
                NOW + 20
            )["ok"],
            true
        );
        assert_eq!(
            send_at(
                &mut store,
                agent_read("t-tasks-8", "run-2", 0, NOW + 10),
                NOW + 20
            )["ok"],
            true
        );
        let forgot = send_at(
            &mut store,
            json!({"op":"forget_person","person":OWNER}),
            NOW + 1_000,
        );
        assert_eq!(forgot["ok"], true, "{forgot}");
        assert!(records(&mut store, OWNER).is_empty());
        assert_eq!(records(&mut store, OTHER).len(), 2);
        let late = send_at(
            &mut store,
            agent_read("t-tasks-7", "run-1", 1, NOW + 500),
            NOW + 2_000,
        );
        assert_eq!(late["error"], "no_owner", "{late}");
        send_at(
            &mut store,
            task_row(OWNER, "t-tasks-7", "run-1", NOW + 1_500),
            NOW + 2_000,
        );
        let early = send_at(
            &mut store,
            agent_read("t-tasks-7", "run-1", 1, NOW + 500),
            NOW + 2_000,
        );
        assert_eq!(early["error"], "forgotten", "{early}");
    }

    /// The voice's `record_source` (slice 7) reads one record by its key:
    /// the person's own alone, whatever its age; after a forget about a
    /// thing only the matching key is gone, and after a forget everything
    /// none is found.
    #[test]
    fn a_record_is_read_by_its_key_and_forgets_take_it() {
        let (_scratch, mut store) = Scratch::new("keyed");
        send(
            &mut store,
            linked(OWNER, "t-s-1", "task", "Booked the dentist", NOW),
        );
        send(
            &mut store,
            linked(OWNER, "t-s-2", "task", "Ordered printer paper", NOW),
        );
        send(&mut store, linked(OTHER, "t-s-3", "task", "Their own", NOW));
        let keyed = |store: &mut Days, person: &str, key: &str| {
            let answer = send(
                store,
                json!({"op":"service_records","person":person,"key":key}),
            );
            assert_eq!(answer["ok"], true, "{answer}");
            answer["records"].as_array().unwrap().clone()
        };
        let one = keyed(&mut store, OWNER, "t-s-1");
        assert_eq!(one.len(), 1);
        assert_eq!(one[0]["source_kind"], "task");
        assert_eq!(one[0]["source_ref"]["service"], "lamp-tasks");
        assert_eq!(one[0]["summary"], "Booked the dentist");
        // Another person's key is not found.
        assert!(keyed(&mut store, OWNER, "t-s-3").is_empty());
        assert!(keyed(&mut store, OWNER, "t-s-9").is_empty());
        send_at(
            &mut store,
            json!({"op":"service_records_forget","person":OWNER,"about":[["dentist"]]}),
            NOW + 1_000,
        );
        assert!(keyed(&mut store, OWNER, "t-s-1").is_empty());
        assert_eq!(keyed(&mut store, OWNER, "t-s-2").len(), 1);
        send_at(
            &mut store,
            json!({"op":"forget_all","person":OWNER}),
            NOW + 2_000,
        );
        assert!(keyed(&mut store, OWNER, "t-s-2").is_empty());
        assert_eq!(keyed(&mut store, OTHER, "t-s-3").len(), 1);
    }

    /// A paired computer's day of notes changes (`sdk`, tracker row C157).
    fn sdk(key: &str, count: u32, deed: u64) -> Value {
        json!({
            "op": "service_record",
            "person": OWNER,
            "key": key,
            "service": "sdk",
            "action": "notes_changed",
            "status": "done",
            "asked_unix_ms": deed,
            "changed_unix_ms": deed,
            "source_kind": "sdk",
            "source_ref": {"service": "sdk", "id": "c_0123456789abcdef", "computer": "Dev laptop"},
            "summary": format!("The paired computer Dev laptop changed its notes {count} times on 2026-11-17"),
            "deed_unix_ms": deed,
        })
    }

    /// One `sdk` row a computer a day: a later put of the same key carries
    /// the higher count, kept in place (done is not final for a count).
    #[test]
    fn an_sdk_row_is_one_a_day_and_its_count_follows_each_put() {
        let (_scratch, mut store) = Scratch::new("sdk-day");
        let first = send(&mut store, sdk("s-c_0123456789abcdef-20261117", 1, NOW));
        assert_eq!(first["ok"], true, "{first}");
        let second = send(
            &mut store,
            sdk("s-c_0123456789abcdef-20261117", 4, NOW + 60_000),
        );
        assert_eq!(second["ok"], true, "{second}");
        let kept = records(&mut store, OWNER);
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert_eq!(kept[0]["source_kind"], "sdk");
        assert_eq!(kept[0]["status"], "done");
        assert!(
            kept[0]["summary"].as_str().unwrap().contains("4 times"),
            "{kept:?}"
        );
        send(
            &mut store,
            sdk("s-c_0123456789abcdef-20261118", 2, NOW + 86_400_000),
        );
        assert_eq!(records(&mut store, OWNER).len(), 2);
    }

    /// An `sdk` row is a count only: any detail, reason, result id, link,
    /// other service, action or status is refused, so no key or value of a
    /// computer's notes can be stored under it.
    #[test]
    fn an_sdk_row_with_anything_but_a_count_is_refused() {
        let (_scratch, mut store) = Scratch::new("sdk-count-only");
        let tries = [
            ("detail", json!({"key": "secret_token"})),
            ("reason", json!("secret_token")),
            ("result_id", json!("secret_token")),
            ("links", json!(["k1"])),
            ("service", json!("gmail")),
            ("action", json!("notes_set")),
            ("status", json!("prepared")),
        ];
        for (field, value) in tries {
            let mut asked = sdk("s-c_0123456789abcdef-20261117", 1, NOW);
            asked[field] = value;
            let answer = send(&mut store, asked);
            assert_eq!(answer["error"], "bad_detail", "{field}: {answer}");
        }
        assert!(records(&mut store, OWNER).is_empty());
    }

    /// Forget everything drops the `sdk` rows, and a count from before the
    /// forget is not kept again (C157, the first builder's 17 Nov).
    #[test]
    fn forget_all_drops_the_sdk_rows() {
        let (_scratch, mut store) = Scratch::new("sdk-forget");
        send(&mut store, sdk("s-c_0123456789abcdef-20261117", 3, NOW));
        assert_eq!(records(&mut store, OWNER).len(), 1);
        let forgot = send_at(
            &mut store,
            json!({"op":"forget_all","person":OWNER}),
            NOW + 1_000,
        );
        assert_eq!(forgot["ok"], true, "{forgot}");
        assert!(records(&mut store, OWNER).is_empty());
        let late = send(&mut store, sdk("s-c_0123456789abcdef-20261117", 3, NOW));
        assert_eq!(late["error"], "forgotten", "{late}");
        assert!(records(&mut store, OWNER).is_empty());
    }

    /// The slice-1 schema (`1dfa0be70a`): `days.sql`, then its four
    /// `ALTER`s, whose `source_kind` CHECK lacks `sdk`, and its index.
    const SLICE_1_ALTERS: &str = "ALTER TABLE service_records ADD COLUMN source_kind TEXT NOT NULL DEFAULT 'voice' CHECK(source_kind IN ('voice','task','agent','reminder'));
        ALTER TABLE service_records ADD COLUMN source_ref TEXT NOT NULL DEFAULT '{}';
        ALTER TABLE service_records ADD COLUMN summary TEXT NOT NULL DEFAULT '';
        ALTER TABLE service_records ADD COLUMN deed_unix_ms INTEGER;
        CREATE INDEX IF NOT EXISTS service_records_kind ON service_records(person,source_kind,changed_unix_ms);";

    fn table_sql(db: &rusqlite::Connection) -> String {
        db.query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='service_records'",
            [],
            |r| r.get(0),
        )
        .unwrap()
    }

    fn table_parts(db: &rusqlite::Connection) -> Vec<(String, String, Option<String>)> {
        let mut statement = db
            .prepare("SELECT type,name,sql FROM sqlite_master WHERE tbl_name='service_records' ORDER BY type,name")
            .unwrap();
        statement
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    }

    /// A database opened on slice 1 (its `source_kind` CHECK without
    /// `sdk`, AppOnly's P2 on #7574) is rebuilt on open: its voice row is
    /// kept as it was, its indexes are made again, and an `sdk` put works.
    /// A second open changes nothing.
    #[test]
    fn a_slice_1_table_takes_an_sdk_row_after_the_open() {
        let (scratch, store) = Scratch::new("slice-1");
        drop(store);
        fs::remove_dir_all(&scratch.0).unwrap();
        crate::store::private_dir(&scratch.0).unwrap();
        {
            let db = database(&scratch);
            db.execute_batch(include_str!("days.sql")).unwrap();
            db.execute_batch(SLICE_1_ALTERS).unwrap();
            assert!(!table_sql(&db).contains("'sdk'"));
            db.execute_batch(&format!(
                "INSERT INTO settings(key,value) VALUES('import_complete',1); PRAGMA user_version=1;
                 INSERT INTO people(person) VALUES('{OWNER}');
                 INSERT INTO service_records(person,key,service,action,status,reason,detail,result_id,asked_unix_ms,changed_unix_ms,links,replaced_by,source_kind,source_ref,summary,deed_unix_ms)
                 VALUES('{OWNER}','old','gmail','send_mail','done','','{{\"subject\":\"Sunday\"}}','',{NOW},{NOW},'[]','','voice','{{}}','Mail to Lan',{NOW});"
            ))
            .unwrap();
        }
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            scratch.0.join(crate::days::DATABASE),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        let mut store = scratch.open();
        let put = send(&mut store, sdk("s-c_0123456789abcdef-20261117", 2, NOW));
        assert_eq!(put["ok"], true, "{put}");
        let kept = records(&mut store, OWNER);
        assert_eq!(kept.len(), 2, "{kept:?}");
        let old = kept.iter().find(|r| r["key"] == "old").unwrap();
        assert_eq!(old["source_kind"], "voice");
        assert_eq!(old["summary"], "Mail to Lan");
        assert_eq!(old["detail"]["subject"], "Sunday");
        assert_eq!(old["deed_unix_ms"], NOW);
        drop(store);
        let db = database(&scratch);
        let once = table_parts(&db);
        assert!(table_sql(&db).contains("'sdk'"), "{once:?}");
        for index in ["service_records_by", "service_records_kind"] {
            assert!(once.iter().any(|(_, name, _)| name == index), "{once:?}");
        }
        drop(db);
        drop(scratch.open());
        let db = database(&scratch);
        assert_eq!(table_parts(&db), once);
        let rows: u64 = db
            .query_row("SELECT count(*) FROM service_records", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 2);
    }

    /// A fresh database's table already allows `sdk`: opening it twice
    /// leaves the table, its indexes and its sql as they were.
    #[test]
    fn a_fresh_table_is_not_rebuilt() {
        let (scratch, mut store) = Scratch::new("fresh-sdk");
        let put = send(&mut store, sdk("s-c_0123456789abcdef-20261117", 1, NOW));
        assert_eq!(put["ok"], true, "{put}");
        drop(store);
        let before = table_parts(&database(&scratch));
        assert!(before.iter().any(
            |(kind, _, sql)| kind == "table" && sql.as_deref().unwrap_or("").contains("'sdk'")
        ));
        drop(scratch.open());
        assert_eq!(table_parts(&database(&scratch)), before);
    }
}
