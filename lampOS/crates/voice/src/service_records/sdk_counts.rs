//! A paired computer's daily notes count (`LAMP_SERVICE_RECORDS_SDK`,
//! tracker row C157, the memory owner's call and the first builder's yes
//! of 17 Nov): one linked record a day for each computer the owner paired
//! from the app, "The paired computer Dev laptop changed its notes 4 times
//! on 2026-11-17", kept in the owner's Days memory. A count only: never a
//! note's key, a key's name or a value.
//!
//! - **Whose.** lamp-control answers the counts (its owner's socket,
//!   `note_changes`) only for the pairings its side file marks as made from
//!   the owner's app (`lamp_control::paired_by`): a keeper's
//!   `lamp-control code`, a stale mark and no side file give nothing. The
//!   row is the owner's (`person::owner_ids`, the first `p-` id); with no
//!   known owner nothing is kept.
//! - **The row.** Key `s-<client>-<YYYYMMDD>` (the lamp's local day),
//!   service `sdk`, action `notes_changed`, status `done`, `source_kind`
//!   `sdk`, `source_ref` `{service: "sdk", id: <client>, computer: <its
//!   name>}`, the summary its one line; no detail. The memory refuses an
//!   `sdk` row with anything else.
//! - **Counted from** the later of the local midnight, the voice's start
//!   and the owner's last forget everything, so a count never reaches back
//!   past a forget; lamp-control's counts are in memory, so a restart of
//!   either starts the day's count again (it may count low, never high).
//!   The deed is the last change counted, so the memory refuses it if a
//!   forget came after it.
//! - **Never slows a turn.** A thread of its own asks every
//!   [`POLL_EVERY`] (1 s at most) and puts only a count higher than the one
//!   it last put, through the linked outbox.
//! - **Off.** Unless `LAMP_SERVICE_RECORDS_SDK=on` beside
//!   `LAMP_SERVICE_RECORDS=on`, nothing is asked, kept or written.
use super::Status;
use super::linked::{Kind, Linked};
use serde_json::{Value, json};
#[cfg(not(test))]
use std::time::Duration;

/// Only the exact value `on`, on the second route, beside
/// `LAMP_SERVICE_RECORDS`.
pub const ENVIRONMENT: &str = "LAMP_SERVICE_RECORDS_SDK";
/// How often the counts are asked.
#[cfg(not(test))]
const POLL_EVERY: Duration = Duration::from_secs(5 * 60);
/// The longest one ask of lamp-control waits, each way.
#[cfg(not(test))]
const ASK_DEADLINE: Duration = Duration::from_secs(1);
/// The most bytes of a computer's name kept in a row.
const NAME_BYTES: usize = 64;
/// The most computers' counts read from one answer: lamp-control keeps 16.
const COMPUTERS_MOST: usize = 16;

#[cfg(not(test))]
mod kept {
    use std::collections::HashMap;
    use std::sync::{LazyLock, Mutex};

    pub fn on() -> bool {
        static ON: LazyLock<bool> =
            LazyLock::new(|| std::env::var(super::ENVIRONMENT).as_deref() == Ok("on"));
        *ON
    }

    /// When the voice started, and the latest forget that counts.
    pub static SINCE: LazyLock<Mutex<u64>> = LazyLock::new(|| Mutex::new(super::super::unix_ms()));
    /// The count last put for each row key.
    pub static PUT: LazyLock<Mutex<HashMap<String, u64>>> = LazyLock::new(Mutex::default);

    pub fn with<R>(f: impl FnOnce(&mut u64, &mut HashMap<String, u64>) -> R) -> R {
        let mut since = SINCE.lock().unwrap_or_else(|p| p.into_inner());
        let mut put = PUT.lock().unwrap_or_else(|p| p.into_inner());
        f(&mut since, &mut put)
    }
}

/// Tests: each test thread its own.
#[cfg(test)]
mod kept {
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;

    thread_local! {
        pub static ON: Cell<bool> = const { Cell::new(false) };
        static SINCE: Cell<u64> = const { Cell::new(0) };
        static PUT: RefCell<HashMap<String, u64>> = RefCell::default();
    }

    pub fn on() -> bool {
        ON.with(Cell::get)
    }

    pub fn with<R>(f: impl FnOnce(&mut u64, &mut HashMap<String, u64>) -> R) -> R {
        SINCE.with(|since| {
            PUT.with(|put| {
                let mut at = since.get();
                let result = f(&mut at, &mut put.borrow_mut());
                since.set(at);
                result
            })
        })
    }
}

/// Whether the counts are kept: this switch and `LAMP_SERVICE_RECORDS`.
pub fn on() -> bool {
    kept::on() && super::on()
}

#[cfg(test)]
pub fn on_for_test(on: bool) {
    kept::ON.with(|cell| cell.set(on));
}

/// A forget of `person`'s everything, or everyone's (`None`), is being
/// sent: counts start again from now, and the next count is put whatever
/// was put before. A forget of another person's changes nothing of the
/// owner's.
pub fn forgot(person: Option<&str>, now_ms: u64) {
    let owners = owner_ids();
    if person.is_some_and(|person| !owners.iter().any(|owner| owner == person)) {
        return;
    }
    kept::with(|since, put| {
        *since = (*since).max(now_ms);
        put.clear();
    });
}

/// The time counts are asked from: the later of `midnight_ms` and the
/// voice's start or last owner's forget.
pub fn since(midnight_ms: u64) -> u64 {
    kept::with(|since, _| (*since).max(midnight_ms))
}

/// The rows of lamp-control's `answer` for `owner` on `day` (`YYYYMMDD`,
/// `day_said` `YYYY-MM-DD`): one for each computer whose count is higher
/// than the one last put, nothing with no known owner. Each row put is
/// remembered.
pub fn rows(answer: &Value, owner: Option<&str>, day: &str, day_said: &str) -> Vec<Linked> {
    let Some(owner) = owner.filter(|owner| lamp_msgs::known::is_person_id(owner)) else {
        return Vec::new();
    };
    let Some(computers) = answer["computers"].as_array() else {
        return Vec::new();
    };
    computers
        .iter()
        .take(COMPUTERS_MOST)
        .filter_map(|computer| {
            let client = computer["client"].as_str()?;
            let changes = computer["changes"].as_u64().filter(|n| *n > 0)?;
            let last = computer["last_unix_ms"].as_u64()?;
            let name = name_of(computer["name"].as_str().unwrap_or(""), client);
            let key = format!("s-{client}-{day}");
            let higher = kept::with(|_, put| {
                let before = put.get(&key).copied().unwrap_or(0);
                (changes > before).then(|| put.insert(key.clone(), changes))
            });
            higher?;
            let times = if changes == 1 { "time" } else { "times" };
            Some(Linked {
                person: owner.to_owned(),
                key,
                kind: Kind::Sdk,
                service: "sdk".to_owned(),
                action: "notes_changed".to_owned(),
                status: Status::Done,
                source_ref: json!({"service": "sdk", "id": client, "computer": name}),
                summary: format!(
                    "The paired computer {name} changed its notes {changes} {times} on {day_said}"
                ),
                deed_ms: last,
                asked_ms: last,
                changed_ms: last,
                links: Vec::new(),
            })
        })
        .collect()
}

/// A computer's name as a row may hold it: no control characters, at most
/// [`NAME_BYTES`] cut at a whole character, its id when nothing is left.
fn name_of(name: &str, client: &str) -> String {
    let clean: String = name.chars().filter(|c| !c.is_control()).collect();
    let mut cut = clean.trim().to_owned();
    while cut.len() > NAME_BYTES {
        cut.pop();
    }
    if cut.is_empty() {
        client.to_owned()
    } else {
        cut
    }
}

#[cfg(not(test))]
fn owner_ids() -> Vec<String> {
    crate::person::owner_ids()
}

#[cfg(test)]
thread_local! {
    static OWNERS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
fn owner_ids() -> Vec<String> {
    OWNERS.with(|owners| owners.borrow().clone())
}

#[cfg(test)]
pub fn owners_for_test(owners: &[&str]) {
    OWNERS.with(|kept| *kept.borrow_mut() = owners.iter().map(|o| (*o).to_owned()).collect());
}

/// The counts' thread, once, with the switch on; off, nothing.
#[cfg(not(test))]
pub fn start() {
    use std::sync::Once;
    static STARTED: Once = Once::new();
    if !on() {
        return;
    }
    STARTED.call_once(|| {
        let _ = kept::with(|since, _| *since);
        eprintln!(
            "lamp-voice second: the owner's app-paired computers' daily notes counts are kept, counts only (LAMP_SERVICE_RECORDS_SDK)"
        );
        let spawned = std::thread::Builder::new()
            .name("sdk-counts".to_owned())
            .spawn(|| {
                loop {
                    std::thread::sleep(POLL_EVERY);
                    poll_once();
                }
            });
        if spawned.is_err() {
            eprintln!("lamp-voice: the notes counts' thread could not start (LAMP_SERVICE_RECORDS_SDK)");
        }
    });
}

#[cfg(test)]
pub fn start() {}

/// One ask of lamp-control and its rows put.
#[cfg(not(test))]
fn poll_once() {
    let now = super::unix_ms();
    let zone = crate::local_time::system();
    let (day, offset_ms) = zone.map_or(((now / 86_400_000) as i64, 0), |zone| {
        (
            zone.day(now),
            i64::from(zone.offset_minutes_at(now)) * 60_000,
        )
    });
    let midnight = u64::try_from(day * 86_400_000 - offset_ms).unwrap_or(0);
    let (year, month, date) = crate::local_time::civil(day);
    let asked = json!({"note_changes": {"since_unix_ms": since(midnight)}});
    let Some(answer) = ask(&asked) else {
        return;
    };
    let owner = owner_ids()
        .into_iter()
        .find(|id| lamp_msgs::known::is_person_id(id));
    let rows = rows(
        &answer,
        owner.as_deref(),
        &format!("{year:04}{month:02}{date:02}"),
        &format!("{year:04}-{month:02}-{date:02}"),
    );
    if !rows.is_empty() {
        eprintln!(
            "lamp-voice: {} computers' notes counts put (LAMP_SERVICE_RECORDS_SDK)",
            rows.len()
        );
    }
    for row in rows {
        super::linked::put(row);
    }
}

/// `asked` on lamp-control's owner's socket, one line each way, at most
/// [`ASK_DEADLINE`] each; none on any failure or an `error` answer (an
/// older lamp-control does not know the op).
#[cfg(not(test))]
fn ask(asked: &Value) -> Option<Value> {
    use std::io::{BufRead, BufReader, Read, Write};
    let stream =
        std::os::unix::net::UnixStream::connect(crate::skill_task_yes::CONTROL_SOCKET).ok()?;
    stream.set_read_timeout(Some(ASK_DEADLINE)).ok()?;
    stream.set_write_timeout(Some(ASK_DEADLINE)).ok()?;
    (&stream).write_all(format!("{asked}\n").as_bytes()).ok()?;
    let mut line = String::new();
    BufReader::new(&stream)
        .take(64 * 1024)
        .read_line(&mut line)
        .ok()?;
    let answer: Value = serde_json::from_str(&line).ok()?;
    if answer.get("error").is_some() {
        eprintln!("lamp-voice: lamp-control gave no notes counts (LAMP_SERVICE_RECORDS_SDK)");
        return None;
    }
    Some(answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNER: &str = "p-0123456789abcdef";
    const OTHER: &str = "p-fedcba9876543210";

    fn answer(changes: u64) -> Value {
        json!({"computers": [
            {"client": "c_0123456789abcdef", "name": "Dev laptop", "changes": changes, "last_unix_ms": 1_000}
        ]})
    }

    /// One row a computer a day, put again only when its count went up.
    #[test]
    fn one_row_a_day_its_count_put_again_only_higher() {
        let first = rows(&answer(1), Some(OWNER), "20261117", "2026-11-17");
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].key, "s-c_0123456789abcdef-20261117");
        assert_eq!(first[0].kind, Kind::Sdk);
        assert_eq!(first[0].person, OWNER);
        assert_eq!(
            first[0].summary,
            "The paired computer Dev laptop changed its notes 1 time on 2026-11-17"
        );
        assert!(rows(&answer(1), Some(OWNER), "20261117", "2026-11-17").is_empty());
        let higher = rows(&answer(4), Some(OWNER), "20261117", "2026-11-17");
        assert_eq!(higher.len(), 1);
        assert_eq!(higher[0].key, first[0].key);
        assert!(higher[0].summary.contains("4 times"));
        let next_day = rows(&answer(1), Some(OWNER), "20261118", "2026-11-18");
        assert_eq!(next_day[0].key, "s-c_0123456789abcdef-20261118");
    }

    /// With no known owner nothing is kept.
    #[test]
    fn no_row_without_a_known_owner() {
        assert!(rows(&answer(3), None, "20261117", "2026-11-17").is_empty());
        assert!(rows(&answer(3), Some("Minh"), "20261117", "2026-11-17").is_empty());
    }

    /// The row holds a count, the computer's id and its name: whatever else
    /// lamp-control's answer carried (a planted key name) is never in it.
    #[test]
    fn no_key_name_is_ever_in_a_row() {
        let planted = json!({"computers": [{
            "client": "c_0123456789abcdef", "name": "Dev\u{7}laptop",
            "changes": 2, "last_unix_ms": 1_000,
            "keys": ["secret_token_name"], "key": "secret_token_name",
            "value": "secret_value"
        }]});
        let rows = rows(&planted, Some(OWNER), "20261117", "2026-11-17");
        assert_eq!(rows.len(), 1);
        let line = rows[0].line().to_string();
        assert!(!line.contains("secret"), "{line}");
        assert!(!line.contains('\u{7}'), "{line}");
        assert_eq!(rows[0].line()["source_kind"], "sdk");
        assert!(rows[0].line().get("detail").is_none());
    }

    /// The owner's forget starts the count again from it and lets the next
    /// count be put; another person's forget changes nothing.
    #[test]
    fn the_owners_forget_moves_the_count_start_and_another_does_not() {
        owners_for_test(&[OWNER]);
        assert_eq!(since(500), 500);
        assert_eq!(
            rows(&answer(2), Some(OWNER), "20261117", "2026-11-17").len(),
            1
        );
        forgot(Some(OTHER), 9_000);
        assert_eq!(since(500), 500);
        assert!(rows(&answer(2), Some(OWNER), "20261117", "2026-11-17").is_empty());
        forgot(Some(OWNER), 9_000);
        assert_eq!(since(500), 9_000);
        assert_eq!(
            rows(&answer(2), Some(OWNER), "20261117", "2026-11-17").len(),
            1
        );
        forgot(None, 12_000);
        assert_eq!(since(500), 12_000);
    }

    /// Off (either switch), the counts are not kept and a row is never put.
    #[test]
    fn off_nothing_is_kept() {
        super::super::on_for_test(true);
        on_for_test(false);
        assert!(!on());
        on_for_test(true);
        super::super::on_for_test(false);
        assert!(!on());
        let row = rows(&answer(1), Some(OWNER), "20261117", "2026-11-17").remove(0);
        assert_eq!(
            super::super::linked::put(row),
            super::super::linked::Put::Off
        );
        super::super::on_for_test(true);
        assert!(on());
    }
}
