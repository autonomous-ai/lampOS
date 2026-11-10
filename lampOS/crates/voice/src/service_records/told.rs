//! Service records told (`LAMP_SERVICE_RECORDS_TOLD`, the connector
//! records plan, PR 1b): the owner's own spoken turns are told their recent
//! service records, one line each, and `recall` searches them all, so "did
//! you do it?" is answered from what happened, never from a guess.
//!
//! On a tester's lamp on 10 Nov a cancelled event was later said to be "on
//! your calendar", and another time "nothing was created… I never had a
//! function to add events": the model knew nothing of either. With the
//! switch, every turn of the owner's is told:
//!
//! - **The latest records.** The latest [`NOTE_SHOWN`], plus any still
//!   `prepared` or `failed` changed within [`OPEN_WITHIN_MS`], latest
//!   first, at most [`NOTE_BYTES`] in all, with [`RULE`] for the model. Read
//!   from what the voice holds, never waited for: after a restart the
//!   owner's records are asked of the memory beside the turns, once
//!   ([`load_ahead`]), and a write left `prepared` by the restart, which
//!   can never be sent now, is `timed_out`.
//! - **Recall.** A recall's words search every record of the owner's in the
//!   memory, whatever its age (`service_records` with `about`), and what it
//!   finds comes back with all its details ([`recall_told`]).
//! - **Who hears.** The services' owner check, not the memory's one-face
//!   rule: only a line that is the owner's (`services::owner_of`, the
//!   check behind `OWNER_ONLY`) is told any; when someone else may be here
//!   (`services::owner_alone`, the check behind `BRIEF_NOTE`), only each
//!   record's service, status and when ([`BRIEF`]).
//! - **Quoted data.** What a read found is the service's data: marked
//!   «…» in the note and kept under `quoted` in recall, each with the
//!   services' `DATA_NOTE`, every time it is shown.
//!
//! Acts only with `LAMP_SERVICE_RECORDS` on too; off, nothing is told and
//! recall is as it was.
use super::{Record, Status, kept, whose};
use serde_json::{Value, json};

/// Only the exact value `on`, with `LAMP_SERVICE_RECORDS` on, on the second
/// route.
pub const ENVIRONMENT: &str = "LAMP_SERVICE_RECORDS_TOLD";

/// The rule for the model, told with the records.
pub const RULE: &str = "Your past service actions are listed with their status. Never claim something was done unless its record says done; never say you can't do something a record shows you did.";
/// What the model is told with the records when someone else may be here.
pub const BRIEF: &str = "Someone else may be here: say only which service it was and how it went, never what it was about.";
/// The latest records told every time.
pub const NOTE_SHOWN: usize = 5;
/// Records still `prepared` or `failed` are told too when changed within
/// this: seven days.
pub const OPEN_WITHIN_MS: u64 = 7 * 86_400_000;
/// The most records told in one note.
pub const NOTE_MOST: usize = 10;
/// The most bytes of the note.
pub const NOTE_BYTES: usize = 1_200;
/// The most records a recall is told.
pub const RECALL_MOST: u64 = 5;
/// The most bytes of the records a recall is told: a function's answer is
/// at most `lamp_gemini::MAX_TOOL_JSON_BYTES` (4 KiB) with what memory
/// found beside them.
pub const RECALL_BYTES: usize = 1_500;
/// The most bytes of the memory's answer of records found.
pub const FOUND_ANSWER_BYTES: usize = 98_304;
/// A text of a write's detail cut to this when the records found are
/// longer than a recall is given.
const CUT_TEXT_BYTES: usize = 200;
/// The most bytes of a subject or a first title in a line.
const SUBJECT_BYTES: usize = 48;
/// The most bytes of a reason in a line.
const REASON_BYTES: usize = 64;

#[cfg(not(test))]
mod switch {
    use std::sync::LazyLock;

    pub fn on() -> bool {
        static ON: LazyLock<bool> =
            LazyLock::new(|| std::env::var(super::ENVIRONMENT).as_deref() == Ok("on"));
        *ON
    }
}

#[cfg(test)]
mod switch {
    use std::cell::Cell;

    thread_local! {
        pub static ON: Cell<bool> = const { Cell::new(false) };
    }

    pub fn on() -> bool {
        ON.with(Cell::get)
    }
}

/// Whether records are told ([`ENVIRONMENT`]): only with records kept.
pub fn on() -> bool {
    switch::on() && super::on()
}

/// Tests: told on or off on this thread.
#[cfg(test)]
pub fn on_for_test(on: bool) {
    switch::ON.with(|cell| cell.set(on));
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// A day as a line says it: "Sun 15 Nov".
fn day_said(days: i64) -> String {
    let (_, month, day) = crate::local_time::civil(days);
    format!(
        "{} {day} {}",
        &crate::local_time::weekday(days)[..3],
        MONTHS[month - 1]
    )
}

/// When `at_ms` was, at `offset` minutes east of UTC, against `now_ms`:
/// "today 15:40", "yesterday 09:10", else "Sat 7 Nov 15:40", and its year
/// when it is not this year's ("Mon 10 Nov 2025 15:40").
fn when(at_ms: u64, now_ms: u64, offset: i32) -> String {
    let local = |ms: u64| i64::try_from(ms / 1_000).unwrap_or(i64::MAX) + i64::from(offset) * 60;
    let (at, now) = (local(at_ms), local(now_ms));
    let (day, seconds) = (at.div_euclid(86_400), at.rem_euclid(86_400));
    let clock = format!("{:02}:{:02}", seconds / 3_600, seconds % 3_600 / 60);
    match now.div_euclid(86_400) - day {
        0 => format!("today {clock}"),
        1 => format!("yesterday {clock}"),
        _ => {
            let year = crate::local_time::civil(day).0;
            if year == crate::local_time::civil(now.div_euclid(86_400)).0 {
                format!("{} {clock}", day_said(day))
            } else {
                format!("{} {year} {clock}", day_said(day))
            }
        }
    }
}

/// An event's time as its write asked it, its own wall clock
/// (`2026-11-15T19:00:00+07:00`): "Sun 15 Nov 19:00–22:00", the end's day
/// said when it is another.
fn event_time(detail: &Value) -> Option<String> {
    let parts = |key: &str| {
        let text = detail.get(key)?.as_str()?;
        let day = crate::local_time::day_of(text.get(..10)?)?;
        let clock = text.get(11..16).filter(|clock| clock.as_bytes()[2] == b':');
        Some((day, clock.map(str::to_owned)))
    };
    let (day, clock) = parts("start")?;
    let mut said = day_said(day);
    if let Some(clock) = &clock {
        said.push(' ');
        said.push_str(clock);
    }
    if let Some((end_day, end_clock)) = parts("end") {
        match (end_day == day, end_clock) {
            (true, Some(end)) if clock.is_some() => said.push_str(&format!("–{end}")),
            (false, Some(end)) => said.push_str(&format!("–{} {end}", day_said(end_day))),
            (false, None) => said.push_str(&format!("–{}", day_said(end_day))),
            _ => {}
        }
    }
    Some(said)
}

/// The name a person hears for a service code.
fn service_said(code: &str) -> String {
    match code {
        "gmail" | "gmail-app" => "Gmail".into(),
        "google_calendar" => "Calendar".into(),
        "google_drive" => "Drive".into(),
        _ => lamp_services::catalog::service(code)
            .map(|service| service.name.to_owned())
            .unwrap_or_else(|| super::tidy(&code.replace(['_', '-'], " "), 32)),
    }
}

fn text_at(detail: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        detail
            .get(*key)
            .and_then(Value::as_str)
            .map(|text| super::tidy(text, SUBJECT_BYTES))
            .filter(|text| !text.is_empty())
    })
}

/// A read's record: what a read keeps (`super::read`, `super::ended`).
fn is_read(record: &Record) -> bool {
    record.status == Status::NoResult
        || record.detail.get("found").is_some()
        || record.detail.get("what").is_some()
}

/// How it went, as a line says it, with its reason in full.
fn status_said(record: &Record, in_full: bool) -> String {
    let status = match record.status {
        Status::Prepared => "waiting for a send",
        Status::Done => "done",
        Status::Cancelled => "cancelled",
        Status::Refused => "refused",
        Status::Failed => "failed",
        Status::TimedOut => "timed out",
        Status::NoResult => "no result",
    };
    let reason = super::tidy(&record.reason, REASON_BYTES);
    if in_full && !reason.is_empty() && !matches!(record.status, Status::Done | Status::NoResult) {
        format!("{status}: {reason}")
    } else {
        status.to_owned()
    }
}

/// One record in one line. In full: "Calendar · create 'Hamburger party' ·
/// Sun 15 Nov 19:00–22:00 · cancelled: the lamp answered after the read-back with no send · today 15:40", a
/// read's first find marked «…» as the service's data. Not in full: its
/// service, how it went and when, nothing it was about.
pub fn line(record: &Record, now_ms: u64, offset: i32, in_full: bool) -> String {
    let service = service_said(&record.service);
    let changed = when(record.changed_ms, now_ms, offset);
    if !in_full {
        return format!("{service} · {} · {changed}", status_said(record, false));
    }
    let mut parts = vec![service];
    let detail = &record.detail;
    if is_read(record) {
        match text_at(detail, &["query"]) {
            Some(query) => parts.push(format!("search '{query}'")),
            None => parts.push(format!("read {}", super::tidy(&record.action, 32))),
        }
        let count = detail.get("count").and_then(Value::as_u64).unwrap_or(0);
        if record.status == Status::Done && count > 0 {
            let first = detail
                .get("found")
                .and_then(|found| found.get(0))
                .and_then(|first| text_at(first, &["title", "from", "date"]));
            parts.push(match first {
                Some(first) => format!("found {count}, first «{first}»"),
                None => format!("found {count}"),
            });
        } else {
            parts.push(status_said(record, true));
        }
    } else {
        let verb = record.action.split('_').next().unwrap_or_default();
        let verb = if verb.is_empty() { "write" } else { verb };
        match text_at(detail, &["title", "subject", "name"]) {
            Some(subject) => parts.push(format!("{verb} '{subject}'")),
            None => parts.push(verb.to_owned()),
        }
        if let Some(time) = event_time(detail) {
            parts.push(time);
        }
        let to = match detail.get("to") {
            Some(Value::String(to)) => Some((to.clone(), 0)),
            Some(Value::Array(to)) => to
                .first()
                .and_then(Value::as_str)
                .map(|first| (first.to_owned(), to.len() - 1)),
            _ => None,
        };
        if let Some((to, more)) = to {
            let to = super::tidy(&to, SUBJECT_BYTES);
            parts.push(if more > 0 {
                format!("to {to} and {more} more")
            } else {
                format!("to {to}")
            });
        }
        parts.push(status_said(record, true));
    }
    parts.push(changed);
    parts.join(" · ")
}

/// `person`'s records told: the latest [`NOTE_SHOWN`], then any older one
/// still `prepared` or `failed` changed within [`OPEN_WITHIN_MS`], latest
/// first, at most [`NOTE_MOST`].
fn chosen(records: &[Record], person: &str, now_ms: u64) -> Vec<Record> {
    let mut theirs: Vec<&Record> = records
        .iter()
        .filter(|record| record.person == person)
        .collect();
    theirs.sort_by(|a, b| {
        (b.changed_ms, b.asked_ms, &b.key).cmp(&(a.changed_ms, a.asked_ms, &a.key))
    });
    theirs
        .iter()
        .enumerate()
        .filter(|(at, record)| {
            *at < NOTE_SHOWN
                || (matches!(record.status, Status::Prepared | Status::Failed)
                    && now_ms.saturating_sub(record.changed_ms) <= OPEN_WITHIN_MS)
        })
        .map(|(_, record)| (*record).clone())
        .take(NOTE_MOST)
        .collect()
}

/// The note for `records` of `person`: [`RULE`], one line a record
/// ([`line`]), and with a read's find shown, the services' `DATA_NOTE`;
/// not in full, [`BRIEF`]. At most [`NOTE_BYTES`]: the oldest lines go
/// first, and how many is said. None when there is none.
pub fn note_of(
    records: &[Record],
    person: &str,
    now_ms: u64,
    offset: i32,
    in_full: bool,
) -> Option<String> {
    let chosen = chosen(records, person, now_ms);
    if chosen.is_empty() {
        return None;
    }
    let head = format!(
        "(For you to know, not to answer unless asked: what you did with the owner's linked services, latest first. {RULE}{}",
        if in_full {
            String::new()
        } else {
            format!(" {BRIEF}")
        }
    );
    let lines: Vec<String> = chosen
        .iter()
        .map(|record| format!("- {}", line(record, now_ms, offset, in_full)))
        .collect();
    let quoted = format!(
        "Words in «» are what a service gave: {}",
        crate::services::DATA_NOTE
    );
    let mut shown = lines.len();
    loop {
        let body = &lines[..shown];
        let marked = body.iter().any(|line| line.contains('«'));
        let mut note = head.clone();
        for line in body {
            note.push('\n');
            note.push_str(line);
        }
        if shown < lines.len() {
            note.push_str(&format!("\n- and {} older", lines.len() - shown));
        }
        if marked {
            note.push('\n');
            note.push_str(&quoted);
        }
        note.push(')');
        if note.len() <= NOTE_BYTES || shown <= 1 {
            return Some(note);
        }
        shown -= 1;
    }
}

/// The note for a line of `owner`'s (the owner's memory id when the line is
/// theirs, `services::owner_of`), at `now_ms` and `offset`; `in_full` when
/// the owner is alone. None with the switch off, for anyone else's line, or
/// with no record. The owner's records are asked of the memory beside it
/// the first time ([`load_ahead`]): never waited for.
pub fn note_now(owner: Option<&str>, in_full: bool, now_ms: u64, offset: i32) -> Option<String> {
    if !on() {
        return None;
    }
    let person = whose(Some(owner?))?;
    load_ahead(&person);
    note_of(&super::held(), &person, now_ms, offset, in_full)
}

/// The memory id the records of a line of `owner`'s (`services::owner_of`)
/// are kept under: theirs, or in the private room the first known owner's.
pub fn person_for(owner: &str) -> Option<String> {
    whose(Some(owner))
}

/// A record as the memory lists it, for `person`.
fn from_memory(person: &str, item: &Value) -> Option<Record> {
    let status = match item["status"].as_str()? {
        "prepared" => Status::Prepared,
        "done" => Status::Done,
        "cancelled" => Status::Cancelled,
        "refused" => Status::Refused,
        "failed" => Status::Failed,
        "timed_out" => Status::TimedOut,
        "no_result" => Status::NoResult,
        _ => return None,
    };
    let text = |key: &str| item[key].as_str().unwrap_or_default().to_owned();
    Some(Record {
        key: item["key"].as_str()?.to_owned(),
        person: person.to_owned(),
        service: text("service"),
        action: text("action"),
        status,
        reason: text("reason"),
        detail: item.get("detail").cloned().unwrap_or_else(|| json!({})),
        result_id: text("result_id"),
        asked_ms: item["asked_unix_ms"].as_u64().unwrap_or(0),
        changed_ms: item["changed_unix_ms"].as_u64().unwrap_or(0),
        send_id: None,
        prepared_at: None,
    })
}

/// The memory's list of `person`'s records taken in after a restart: each
/// one the voice does not hold already, oldest first under what it holds;
/// a write still `prepared` from before, which no send can reach now,
/// `timed_out` and written so. What is held stays as it is. Nothing is
/// taken in when a forget ran since the list was asked (`forgets_before`,
/// [`super::forgets`] read before the ask).
pub fn merge(person: &str, answer: &Value, now_ms: u64, forgets_before: u64) {
    if answer["ok"] != true {
        return;
    }
    let listed: Vec<Record> = answer["records"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| from_memory(person, item))
        .collect();
    let changes: Vec<Record> = kept::ledger(|ledger| {
        let mut changes = Vec::new();
        // A forget ran since the list was asked: it may hold what was
        // forgotten, so none of it is taken in.
        if ledger.forgets != forgets_before {
            return changes;
        }
        // The list is the latest first: each put in front, the oldest last.
        for mut record in listed {
            if ledger
                .records
                .iter()
                .any(|held| held.key == record.key && held.person == record.person)
            {
                continue;
            }
            if record.status == Status::Prepared {
                record.status = Status::TimedOut;
                record.reason = "no send came before the lamp restarted".into();
                record.changed_ms = now_ms.max(record.asked_ms);
                changes.push(record.clone());
            }
            ledger.records.push_front(record);
        }
        // Held past what the voice keeps: the oldest final ones go.
        while ledger.records.len() > super::RECORDS_KEPT {
            let at = ledger
                .records
                .iter()
                .position(|record| record.status != Status::Prepared)
                .unwrap_or(0);
            let Some(gone) = ledger.records.remove(at) else {
                break;
            };
            for receipt in &mut ledger.receipts {
                if receipt.record.as_deref() == Some(gone.key.as_str()) {
                    receipt.record = None;
                }
            }
        }
        changes
    });
    changes.into_iter().for_each(super::save);
}

/// The memory's request for `person`'s records about a recall's words.
pub fn about_request(person: &str, about: &str) -> Value {
    json!({"op":"service_records","person":person,"about":about,"limit":RECALL_MOST})
}

/// What a recall is told of the records the memory found (`answer`), or
/// none. In full: each with its line, status, reason, when, what it was
/// asked with (a write's in full) and, of a read, what it found under
/// `quoted` with the services' `DATA_NOTE`. Not in full: each one's
/// service, status and when, and [`BRIEF`]. At most `most` bytes: past it
/// each long text is cut (and said `cut`), then the last records go; none
/// fits, none.
pub fn recall_told(
    answer: &Value,
    in_full: bool,
    now_ms: u64,
    offset: i32,
    most: usize,
) -> Option<Value> {
    if answer["ok"] != true {
        return None;
    }
    let records: Vec<Value> = answer["records"]
        .as_array()?
        .iter()
        .filter_map(|item| from_memory("", item))
        .map(|record| {
            if !in_full {
                return json!({
                    "summary": line(&record, now_ms, offset, false),
                    "service": service_said(&record.service),
                    "status": record.status.wire(),
                    "changed": when(record.changed_ms, now_ms, offset),
                });
            }
            let mut detail = record.detail.clone();
            let found = detail
                .as_object_mut()
                .and_then(|detail| detail.remove("found"));
            let mut told = json!({
                "summary": line(&record, now_ms, offset, true),
                "service": record.service,
                "action": record.action,
                "status": record.status.wire(),
                "reason": record.reason,
                "asked": when(record.asked_ms, now_ms, offset),
                "changed": when(record.changed_ms, now_ms, offset),
                "detail": detail,
            });
            if !record.result_id.is_empty() {
                told["result_id"] = json!(record.result_id);
            }
            if let Some(found) = found {
                told["quoted"] = json!({"found": found, "note": crate::services::DATA_NOTE});
            }
            told
        })
        .collect();
    let told_of = |records: &[Value]| {
        let mut told = json!({"records": records, "note": RULE});
        if !in_full {
            told["others_here"] = json!(BRIEF);
        }
        told
    };
    let mut records = records;
    if told_of(&records).to_string().len() > most {
        for record in &mut records {
            cut_texts(record);
        }
    }
    while !records.is_empty() && told_of(&records).to_string().len() > most {
        records.pop();
    }
    (!records.is_empty()).then(|| told_of(&records))
}

/// Each text of a record told to a recall cut to [`CUT_TEXT_BYTES`], the
/// cut said.
fn cut_texts(record: &mut Value) {
    fn cut(value: &mut Value) -> bool {
        let mut any = false;
        match value {
            Value::String(text) if text.len() > CUT_TEXT_BYTES => {
                *text = super::tidy(text, CUT_TEXT_BYTES);
                any = true;
            }
            Value::Array(items) => {
                for item in items {
                    any |= cut(item);
                }
            }
            Value::Object(object) => {
                for item in object.values_mut() {
                    any |= cut(item);
                }
            }
            _ => {}
        }
        any
    }
    let mut any = false;
    for key in ["detail", "quoted"] {
        if let Some(value) = record.get_mut(key) {
            any |= cut(value);
        }
    }
    if any {
        record["cut"] = json!(true);
    }
}

/// The owner's records asked of the memory once after a start, beside the
/// turns ([`merge`]); a failed ask is asked again at the next turn.
#[cfg(not(test))]
pub fn load_ahead(person: &str) {
    use std::{
        collections::HashSet,
        sync::{LazyLock, Mutex},
    };
    static ASKED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Mutex::default);
    let Ok(mut asked) = ASKED.lock() else {
        return;
    };
    if !asked.insert(person.to_owned()) {
        return;
    }
    drop(asked);
    let person = person.to_owned();
    let again = person.clone();
    let spawned = std::thread::Builder::new()
        .name("service-records-told".into())
        .spawn(move || {
            // At a start the memory's client may not be made yet: a few
            // seconds for it, off every turn.
            let mut socket = crate::remember::days_socket();
            for _ in 0..10 {
                if socket.is_some() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
                socket = crate::remember::days_socket();
            }
            let forgets = super::forgets();
            let answer = socket.and_then(|socket| {
                crate::tools::exchange(
                    &socket,
                    &json!({"op":"service_records","person":person,"limit":crate::service_records::RECORDS_KEPT}),
                    std::time::Duration::from_secs(2),
                )
            });
            match answer.filter(|answer| answer["ok"] == true) {
                Some(answer) => {
                    let count = answer["records"].as_array().map_or(0, Vec::len);
                    merge(&person, &answer, super::unix_ms(), forgets);
                    eprintln!(
                        "lamp-voice: {count} service records of the owner's taken from the memory (LAMP_SERVICE_RECORDS_TOLD)"
                    );
                }
                None => {
                    eprintln!(
                        "lamp-voice: the memory did not list the owner's service records; asked again at the next turn (LAMP_SERVICE_RECORDS_TOLD)"
                    );
                    if let Ok(mut asked) = ASKED.lock() {
                        asked.remove(&person);
                    }
                }
            }
        });
    if spawned.is_err()
        && let Ok(mut asked) = ASKED.lock()
    {
        asked.remove(&again);
    }
}

#[cfg(test)]
pub fn load_ahead(_person: &str) {}

#[cfg(test)]
mod tests;
