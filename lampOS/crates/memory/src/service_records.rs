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
//!
//! What a read found is stored by the voice as metadata and a short masked
//! excerpt only; this store keeps what it is given and checks only sizes
//! and shapes.
use crate::{
    days::{Days, Error, Result, ensure_person},
    store::{self, Person},
};
use rusqlite::{OptionalExtension, Transaction, params};
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
}

fn key_ok(key: &str) -> bool {
    (1..=64).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn owner_only(person: &Person) -> Result<()> {
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
fn folded(text: &str) -> String {
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
fn has_word(words: &[&str], word: &str) -> bool {
    let word = singular(word);
    words.iter().any(|said| singular(said) == word)
}

/// `about` (each subject's words) folded, the empty words and subjects
/// left out.
fn subjects_of(about: &[Vec<String>]) -> Vec<Vec<String>> {
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
fn about_any(subjects: &[Vec<String>], text: &str) -> bool {
    let text = folded(text);
    let words: Vec<&str> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    subjects
        .iter()
        .any(|subject| subject.iter().all(|word| has_word(&words, word)))
}

/// The record's words, as a forget about a thing matches them.
fn text_of(service: &str, action: &str, reason: &str, result_id: &str, detail: &Value) -> String {
    let mut text = format!("{service} {action} {reason} {result_id} ");
    words_of(detail, &mut text);
    text
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
        self.refuse_forgotten(person, record, || {
            text_of(
                &service,
                &action,
                &reason,
                &result_id,
                record.detail.as_ref().unwrap_or(&Value::Null),
            )
        })?;
        let links = json!(record.links).to_string();
        let tx = self.db.transaction()?;
        ensure_person(&tx, person)?;
        let kept: Option<String> = tx
            .query_row(
                "SELECT status FROM service_records WHERE person=?1 AND key=?2",
                params![person.as_str(), record.key],
                |r| r.get(0),
            )
            .optional()?;
        let status = match kept {
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
                    "UPDATE service_records SET service=?3,action=?4,status=?5,reason=?6,detail=?7,result_id=?8,changed_unix_ms=max(changed_unix_ms,?9),links=?10,replaced_by=?11 WHERE person=?1 AND key=?2",
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
                        record.replaced_by
                    ],
                )?;
                record.status.clone()
            }
            None => {
                tx.execute(
                    "INSERT INTO service_records(person,key,service,action,status,reason,detail,result_id,asked_unix_ms,changed_unix_ms,links,replaced_by) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
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
                        record.replaced_by
                    ],
                )?;
                trim(&tx, person)?;
                record.status.clone()
            }
        };
        tx.commit()?;
        Ok(status)
    }

    /// A record asked before the latest forget of `person`'s everything
    /// (or everyone's), or before a forget about a thing it matches, is
    /// refused: a change the voice sent before the forget and this store
    /// took after it is never kept (Jarvis's P2 on #6073). `text`: the
    /// record's words.
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
        let mark: Option<i64> = self.db.query_row(
            "SELECT max(at_unix_ms) FROM service_records_forgot WHERE person IN (?1,'')",
            [person.as_str()],
            |r| r.get(0),
        )?;
        if mark.is_some_and(|mark| u64::try_from(mark).unwrap_or(0) > record.asked_unix_ms) {
            return refused();
        }
        let about: Vec<&Vec<Vec<String>>> = self
            .records_forgot_about
            .iter()
            .filter(|(whose, _, at)| whose == person.as_str() && *at > record.asked_unix_ms)
            .map(|(_, subjects, _)| subjects)
            .collect();
        if about.is_empty() {
            return Ok(());
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

    /// `person`'s records whose words match `about` (a recall's words, the
    /// voice's `LAMP_SERVICE_RECORDS_TOLD`), whatever their age: the most
    /// words matched first, then the latest change; at most `limit` (and
    /// [`LIST_MOST`]). A word matches a record's word lower case, without
    /// accents, as its singular, or as the start of a longer one
    /// ("octavia" finds `octaviabmu@gmail.com`); small words ("the",
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
                "SELECT key, service||' '||action||' '||reason||' '||result_id, detail, changed_unix_ms, asked_unix_ms FROM service_records WHERE person=?1",
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
            let mut statement = tx.prepare(
                "SELECT key, service||' '||action||' '||reason||' '||result_id, detail FROM service_records WHERE person=?1",
            )?;
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
const COLUMNS: &str = "key,service,action,status,reason,detail,result_id,asked_unix_ms,changed_unix_ms,links,replaced_by";

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
            "detail":{"to":["octaviabmu@gmail.com"],"subject":"Đi chơi Chủ nhật","text":"See you"},
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
            "detail":{"to":["octaviabmu@gmail.com"],"subject":"Sunday","text":"Hi Octavia, see you Sunday"},
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
            };
            store.service_record_put(&person, &record).unwrap();
        }
        let listed = store.service_records(&person, 2).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0]["key"], "k2");
        assert_eq!(listed[1]["key"], "k1");
    }
}
