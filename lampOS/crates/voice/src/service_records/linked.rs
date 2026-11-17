//! The linked outbox (the linked-service memory plan, slice 2): the
//! voice's queue of linked records, a Harness task, an agent's write or a
//! reminder kept for the person it was for, written to the Days memory as
//! a `service_record` with its `source_kind`, `source_ref`, `summary` and
//! `deed_unix_ms` (memory-days.md, "Source kinds, pointers, summaries and
//! deed times").
//!
//! - **Its own bound.** At most [`LINKED_MOST`] changes wait, apart from
//!   the connector records' [`super::OUTBOX_MOST`], so a burst of task or
//!   reminder news never pushes out a send's record. Full, the oldest goes,
//!   counted ([`dropped`], `linked_dropped` in the voice's health) and said
//!   in one journal line, its key and kind only.
//! - **Never waits.** [`put`] only takes the queue's lock, held by the
//!   writer to look at the front and to take it off, never across a write:
//!   the writer is a thread of its own, as the connector records' is.
//! - **Whose.** Only a known person's (`p-` ids): with no person, or one
//!   not known, nothing is kept or written ([`Put::NoPerson`]). Unlike a
//!   connector record, there is no fallback to the lamp's owner: a linked
//!   record is saved for the person the voice mapped it to, or not at all.
//! - **Forgets win.** A forget's changes still waiting go before the
//!   forget is sent ([`unsent`], [`unsent_about`], called by the connector
//!   records' own), and the memory refuses a deed from before a forget.
//! - **Off.** With `LAMP_SERVICE_RECORDS` off nothing is kept
//!   ([`Put::Off`]). Each writer comes with its own switch: Harness task
//!   rows (`LAMP_SERVICE_RECORDS_TASKS`, [`super::task_rows`]); reminder
//!   rows (`LAMP_SERVICE_RECORDS_REMINDERS`, [`super::reminder_rows`]); a
//!   paired computer's daily notes count (`LAMP_SERVICE_RECORDS_SDK`,
//!   [`super::sdk_counts`]); agent rows (`LAMP_SERVICE_RECORDS_AGENT`,
//!   [`super::agent_rows`]).
use super::{Status, tidy};
use serde_json::{Value, json};
use std::collections::VecDeque;
#[cfg(not(test))]
use std::time::Duration;

/// The most linked changes waiting to be written.
pub const LINKED_MOST: usize = 16;
/// The most bytes of a summary, as the memory keeps it.
pub const SUMMARY_BYTES: usize = 160;
/// The most bytes of a `source_ref`, as JSON.
pub const SOURCE_REF_BYTES: usize = 512;
/// The longest one write to the memory waits.
#[cfg(not(test))]
const SAVE_DEADLINE: Duration = Duration::from_secs(2);
/// The longest wait before a write the memory did not answer is tried
/// again.
#[cfg(not(test))]
const RETRY_MOST: Duration = Duration::from_secs(30);

/// Where a linked record's thing lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Task,
    Agent,
    Reminder,
    /// A paired computer's daily notes count ([`super::sdk_counts`]).
    Sdk,
}

impl Kind {
    pub fn wire(self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Agent => "agent",
            Self::Reminder => "reminder",
            Self::Sdk => "sdk",
        }
    }
}

/// One linked record change, as the voice queues it.
#[derive(Clone, Debug, PartialEq)]
pub struct Linked {
    pub person: String,
    pub key: String,
    pub kind: Kind,
    pub service: String,
    pub action: String,
    pub status: Status,
    /// `{service, id, session?, computer?}`.
    pub source_ref: Value,
    pub summary: String,
    pub deed_ms: u64,
    pub asked_ms: u64,
    pub changed_ms: u64,
    pub links: Vec<String>,
}

impl Linked {
    /// The memory's `service_record` request for it.
    pub fn line(&self) -> Value {
        json!({
            "op": "service_record",
            "person": self.person,
            "key": self.key,
            "service": self.service,
            "action": self.action,
            "status": self.status.wire(),
            "asked_unix_ms": self.asked_ms,
            "changed_unix_ms": self.changed_ms,
            "links": self.links,
            "source_kind": self.kind.wire(),
            "source_ref": self.source_ref,
            "summary": self.summary,
            "deed_unix_ms": self.deed_ms,
        })
    }
}

/// What [`put`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Put {
    /// It waits to be written (a waiting change of the same key replaced).
    Waiting,
    /// It waits, and the oldest waiting change went to make room.
    DroppedOldest,
    /// No known person: nothing kept.
    NoPerson,
    /// A key, service or `source_ref` the memory would refuse: nothing kept.
    Bad,
    /// `LAMP_SERVICE_RECORDS` is off: nothing kept.
    Off,
}

/// The linked changes waiting, oldest first: one a key, the latest.
#[derive(Default)]
pub struct Outbox {
    waiting: VecDeque<Linked>,
    /// Whether the memory did not answer the last write.
    down: bool,
}

#[cfg(not(test))]
mod kept {
    use super::Outbox;
    use std::sync::{
        Condvar, LazyLock, Mutex,
        atomic::{AtomicU64, Ordering},
    };

    pub static OUTBOX: LazyLock<(Mutex<Outbox>, Condvar)> =
        LazyLock::new(|| (Mutex::default(), Condvar::new()));
    static DROPPED: AtomicU64 = AtomicU64::new(0);

    pub fn outbox<R>(f: impl FnOnce(&mut Outbox) -> R) -> R {
        let result = f(&mut OUTBOX
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()));
        OUTBOX.1.notify_one();
        result
    }

    pub fn count_drop() -> u64 {
        DROPPED.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub fn dropped() -> u64 {
        DROPPED.load(Ordering::Relaxed)
    }
}

/// Tests: each test thread its own, so parallel tests never meet.
#[cfg(test)]
mod kept {
    use super::Outbox;
    use std::cell::{Cell, RefCell};

    thread_local! {
        static OUTBOX: RefCell<Outbox> = RefCell::default();
        static DROPPED: Cell<u64> = const { Cell::new(0) };
    }

    pub fn outbox<R>(f: impl FnOnce(&mut Outbox) -> R) -> R {
        OUTBOX.with(|cell| f(&mut cell.borrow_mut()))
    }

    pub fn count_drop() -> u64 {
        DROPPED.with(|cell| {
            cell.set(cell.get() + 1);
            cell.get()
        })
    }

    pub fn dropped() -> u64 {
        DROPPED.with(Cell::get)
    }
}

/// A key the memory takes: 1 to 64 letters, digits, `-` and `_`.
fn key_ok(key: &str) -> bool {
    (1..=64).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// A `source_ref` the memory takes: an object of `service` and `id` (texts,
/// not empty) and, beside them, only `session` and `computer` (texts), no
/// control characters, at most [`SOURCE_REF_BYTES`] as JSON.
fn source_ref_ok(source_ref: &Value) -> bool {
    let Some(object) = source_ref.as_object() else {
        return false;
    };
    let text_ok = |value: &Value| {
        value
            .as_str()
            .is_some_and(|text| !text.chars().any(char::is_control))
    };
    ["service", "id"].iter().all(|key| {
        object
            .get(*key)
            .is_some_and(|value| text_ok(value) && value.as_str().is_some_and(|t| !t.is_empty()))
    }) && object.iter().all(|(key, value)| {
        matches!(key.as_str(), "service" | "id" | "session" | "computer") && text_ok(value)
    }) && source_ref.to_string().len() <= SOURCE_REF_BYTES
}

/// `change` queued for the Days memory, for the known person it names: a
/// waiting change of the same person and key is replaced; full, the oldest
/// goes, counted and said. Never waits for the memory.
pub fn put(mut change: Linked) -> Put {
    if !super::on() {
        return Put::Off;
    }
    if !lamp_msgs::known::is_person_id(&change.person) {
        return Put::NoPerson;
    }
    if !key_ok(&change.key)
        || change.service.trim().is_empty()
        || change.action.trim().is_empty()
        || !source_ref_ok(&change.source_ref)
    {
        return Put::Bad;
    }
    change.summary = tidy(&change.summary, SUMMARY_BYTES);
    change.changed_ms = change.changed_ms.max(change.asked_ms);
    eprintln!(
        "lamp-voice: linked record {}: {} {} {} {} (LAMP_SERVICE_RECORDS)",
        change.key,
        change.kind.wire(),
        change.service,
        change.action,
        change.status.wire()
    );
    let gone = kept::outbox(|outbox| {
        if let Some(waiting) = outbox
            .waiting
            .iter_mut()
            .find(|waiting| waiting.key == change.key && waiting.person == change.person)
        {
            *waiting = change;
            return None;
        }
        let gone = if outbox.waiting.len() >= LINKED_MOST {
            outbox.waiting.pop_front()
        } else {
            None
        };
        outbox.waiting.push_back(change);
        gone
    });
    start_saver();
    match gone {
        None => Put::Waiting,
        Some(gone) => {
            let count = kept::count_drop();
            eprintln!(
                "lamp-voice: linked record {} ({}) was not saved: {LINKED_MOST} wait already; linked_dropped={count} (LAMP_SERVICE_RECORDS)",
                gone.key,
                gone.kind.wire()
            );
            Put::DroppedOldest
        }
    }
}

/// How many linked changes went unsaved because the outbox was full, since
/// the voice started: `linked_dropped` in its health.
pub fn dropped() -> u64 {
    kept::dropped()
}

/// The linked changes waiting, oldest first.
pub fn waiting() -> Vec<Linked> {
    kept::outbox(|outbox| outbox.waiting.iter().cloned().collect())
}

/// A forget of `person`'s everything, or everyone's (`None`), is about to
/// be sent, or was answered: every linked change of theirs still waiting
/// goes.
pub fn unsent(person: Option<&str>) {
    kept::outbox(|outbox| {
        outbox
            .waiting
            .retain(|change| person.is_some_and(|person| change.person != person));
    });
    super::task_rows::forgot(person);
    super::reminder_rows::forgot(person);
    super::agent_rows::forgot(person);
    super::sdk_counts::forgot(person, super::unix_ms());
}

/// A forget about a thing of `person`'s is about to be sent, or was
/// answered: the linked changes waiting whose service, action or summary
/// `matches` go.
pub fn unsent_about(person: &str, matches: impl Fn(&str) -> bool) {
    super::task_rows::forgot_about(person, &matches);
    super::reminder_rows::forgot_about(person, &matches);
    super::agent_rows::forgot_about(person, &matches);
    kept::outbox(|outbox| {
        outbox.waiting.retain(|change| {
            change.person != person
                || !matches(&format!(
                    "{} {} {}",
                    change.service, change.action, change.summary
                ))
        });
    });
}

/// Writes the oldest waiting change with `send`, the memory's exchange,
/// the lock not held while it waits: written or refused, it goes; no
/// answer, it stays first, and the journal says so once until the memory
/// answers again.
pub fn save_next(send: impl FnOnce(&Value) -> Option<Value>) -> super::Saved {
    use super::Saved;
    let Some(change) = kept::outbox(|outbox| outbox.waiting.front().cloned()) else {
        return Saved::Nothing;
    };
    let answer = send(&change.line());
    kept::outbox(|outbox| {
        let Some(answer) = answer else {
            if !outbox.down {
                eprintln!(
                    "lamp-voice: the memory did not take a linked record: {} wait, tried again (LAMP_SERVICE_RECORDS)",
                    outbox.waiting.len()
                );
            }
            outbox.down = true;
            return Saved::Down;
        };
        outbox.down = false;
        // The one sent, unless a forget took it or a newer change of it
        // came meanwhile: that is written next.
        if let Some(at) = outbox.waiting.iter().position(|waiting| *waiting == change) {
            outbox.waiting.remove(at);
        }
        if answer["ok"] != true {
            eprintln!(
                "lamp-voice: linked record {} was refused by the memory: {} (LAMP_SERVICE_RECORDS)",
                change.key,
                answer["error"].as_str().unwrap_or("no reason")
            );
        }
        Saved::Done
    })
}

/// The writer: a thread of its own, started at the first change, apart
/// from the connector records' so neither waits on the other's backlog;
/// memory down, it tries again after 1 s, doubling to [`RETRY_MOST`].
#[cfg(not(test))]
fn start_saver() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        let spawned = std::thread::Builder::new()
            .name("linked-records".into())
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
                    let Some(socket) = crate::remember::days_socket() else {
                        wait = RETRY_MOST;
                        continue;
                    };
                    loop {
                        match save_next(|line| {
                            crate::tools::exchange(&socket, line, SAVE_DEADLINE)
                        }) {
                            super::Saved::Done => wait = Duration::from_secs(1),
                            super::Saved::Nothing => break,
                            super::Saved::Down => {
                                wait = (wait * 2).min(RETRY_MOST);
                                break;
                            }
                        }
                    }
                }
            });
        if spawned.is_err() {
            eprintln!(
                "lamp-voice: the linked records writer did not start; linked records are not saved (LAMP_SERVICE_RECORDS)"
            );
        }
    });
}

#[cfg(test)]
fn start_saver() {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service_records::{Saved, on_for_test};

    const OWNER: &str = "p-0123456789abcdef";
    const OTHER: &str = "p-fedcba9876543210";

    fn change(person: &str, key: &str) -> Linked {
        Linked {
            person: person.into(),
            key: key.into(),
            kind: Kind::Task,
            service: "harness".into(),
            action: "task".into(),
            status: Status::Done,
            source_ref: json!({"service":"lamp-tasks","id":"7","session":"run-1"}),
            summary: "Booked the dentist for Tuesday".into(),
            deed_ms: 1_000,
            asked_ms: 1_000,
            changed_ms: 2_000,
            links: Vec::new(),
        }
    }

    /// Off, nothing is kept, whoever it is for.
    #[test]
    fn off_nothing_is_kept() {
        on_for_test(false);
        assert_eq!(put(change(OWNER, "t-tasks-1")), Put::Off);
        assert!(waiting().is_empty());
        assert_eq!(dropped(), 0);
    }

    /// With no person, or one not known, nothing is saved: no fallback to
    /// the lamp's owner.
    #[test]
    fn no_known_person_means_nothing_saved() {
        on_for_test(true);
        for person in ["", "everyone", "room", "guest-1", "p-short"] {
            assert_eq!(put(change(person, "t-tasks-1")), Put::NoPerson, "{person}");
        }
        assert!(waiting().is_empty());
        assert_eq!(dropped(), 0);
    }

    /// Full, the oldest goes and is counted; the newest waits; it never
    /// waits for the memory (no writer runs in this test, and every put
    /// returns at once).
    #[test]
    fn full_it_drops_the_oldest_counted_and_never_blocks() {
        on_for_test(true);
        for n in 0..LINKED_MOST {
            assert_eq!(put(change(OWNER, &format!("t-tasks-{n}"))), Put::Waiting);
        }
        assert_eq!(dropped(), 0);
        assert_eq!(
            put(change(OWNER, &format!("t-tasks-{LINKED_MOST}"))),
            Put::DroppedOldest
        );
        assert_eq!(put(change(OWNER, "t-tasks-x")), Put::DroppedOldest);
        let keys: Vec<String> = waiting().into_iter().map(|change| change.key).collect();
        assert_eq!(keys.len(), LINKED_MOST);
        assert_eq!(keys[0], "t-tasks-2");
        assert_eq!(keys[LINKED_MOST - 1], "t-tasks-x");
        assert_eq!(dropped(), 2);
    }

    /// The connector records' outbox is apart: a full linked outbox leaves
    /// it as it was.
    #[test]
    fn it_is_apart_from_the_connector_records_outbox() {
        on_for_test(true);
        for n in 0..LINKED_MOST + 4 {
            put(change(OWNER, &format!("t-tasks-{n}")));
        }
        assert!(crate::service_records::waiting().is_empty());
    }

    /// A newer change of the same key replaces the one waiting, and is not
    /// a drop.
    #[test]
    fn a_newer_change_of_a_key_replaces_it() {
        on_for_test(true);
        put(change(OWNER, "r-9-set"));
        let mut newer = change(OWNER, "r-9-set");
        newer.status = Status::Failed;
        assert_eq!(put(newer), Put::Waiting);
        let waiting = waiting();
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].status, Status::Failed);
        assert_eq!(dropped(), 0);
    }

    /// A key, service or pointer the memory would refuse is never queued.
    #[test]
    fn a_bad_record_is_not_queued() {
        on_for_test(true);
        let mut bad_key = change(OWNER, "t tasks 1");
        assert_eq!(put(bad_key.clone()), Put::Bad);
        bad_key.key = "k".repeat(65);
        assert_eq!(put(bad_key), Put::Bad);
        for source_ref in [
            json!("lamp-tasks"),
            json!({"service":"lamp-tasks"}),
            json!({"service":"","id":"7"}),
            json!({"service":"lamp-tasks","id":"7","text":"the mail"}),
            json!({"service":"lamp-tasks","id":"7\n"}),
            json!({"service":"lamp-tasks","id":"x".repeat(600)}),
        ] {
            let mut bad = change(OWNER, "t-tasks-1");
            bad.source_ref = source_ref.clone();
            assert_eq!(put(bad), Put::Bad, "{source_ref}");
        }
        let mut no_service = change(OWNER, "t-tasks-1");
        no_service.service = " ".into();
        assert_eq!(put(no_service), Put::Bad);
        assert!(waiting().is_empty());
    }

    /// The summary is tidied to one line of at most 160 bytes, cut at a
    /// whole character.
    #[test]
    fn the_summary_is_one_line_of_at_most_160_bytes() {
        on_for_test(true);
        let mut long = change(OWNER, "t-tasks-1");
        long.summary = format!("Đặt\nlịch {}", "nha sĩ ".repeat(40));
        put(long);
        let summary = &waiting()[0].summary;
        assert!(summary.len() <= SUMMARY_BYTES, "{}", summary.len());
        assert!(summary.starts_with("Đặt lịch nha"), "{summary}");
        assert!(!summary.contains('\n'));
    }

    /// The line is the memory's `service_record` with the linked fields.
    #[test]
    fn the_line_is_a_linked_service_record() {
        on_for_test(true);
        put(change(OWNER, "t-tasks-7"));
        let line = waiting()[0].line();
        assert_eq!(line["op"], "service_record");
        assert_eq!(line["person"], OWNER);
        assert_eq!(line["key"], "t-tasks-7");
        assert_eq!(line["source_kind"], "task");
        assert_eq!(line["source_ref"]["session"], "run-1");
        assert_eq!(line["summary"], "Booked the dentist for Tuesday");
        assert_eq!(line["deed_unix_ms"], 1_000);
        assert_eq!(line["status"], "done");
        assert!(line.get("detail").is_none());
    }

    /// The writer sends with the queue's lock free: a put made while a
    /// write waits for the memory goes through (here, from inside the
    /// send, which would panic on a held lock), and is written next.
    #[test]
    fn a_put_while_a_write_waits_is_not_held() {
        on_for_test(true);
        put(change(OWNER, "t-tasks-1"));
        let saved = save_next(|line| {
            assert_eq!(line["key"], "t-tasks-1");
            assert_eq!(put(change(OWNER, "t-tasks-2")), Put::Waiting);
            Some(json!({"ok":true,"status":"kept"}))
        });
        assert_eq!(saved, Saved::Done);
        let keys: Vec<String> = waiting().into_iter().map(|change| change.key).collect();
        assert_eq!(keys, ["t-tasks-2"]);
    }

    /// Memory down, the change stays first and is tried again; refused, it
    /// goes; nothing waiting, nothing is sent.
    #[test]
    fn down_it_waits_and_refused_it_goes() {
        on_for_test(true);
        assert_eq!(save_next(|_| panic!("nothing waits")), Saved::Nothing);
        put(change(OWNER, "t-tasks-1"));
        assert_eq!(save_next(|_| None), Saved::Down);
        assert_eq!(waiting().len(), 1);
        assert_eq!(
            save_next(|_| Some(json!({"ok":false,"error":"forgotten"}))),
            Saved::Done
        );
        assert!(waiting().is_empty());
    }

    /// A forget's changes still waiting go: everything of the person's,
    /// everyone's, or those about a thing; another person's stay.
    #[test]
    fn a_forget_takes_the_waiting_changes() {
        on_for_test(true);
        put(change(OWNER, "t-tasks-1"));
        let mut other = change(OTHER, "t-tasks-2");
        other.summary = "Watered the plants".into();
        put(other);
        let mut reminder = change(OWNER, "r-3-set");
        reminder.kind = Kind::Reminder;
        reminder.summary = "Call mum at six".into();
        put(reminder);

        crate::service_records::unsent_about(OWNER, &[vec!["mum".into()]]);
        let keys: Vec<String> = waiting().into_iter().map(|change| change.key).collect();
        assert_eq!(keys, ["t-tasks-1", "t-tasks-2"]);

        crate::service_records::unsent(OWNER);
        let keys: Vec<String> = waiting().into_iter().map(|change| change.key).collect();
        assert_eq!(keys, ["t-tasks-2"]);

        crate::service_records::forgot(None);
        assert!(waiting().is_empty());
    }

    /// A forget about a thing answered by the memory takes the matching
    /// changes too, as the connector records' does.
    #[test]
    fn a_forget_about_answered_takes_the_matching_changes() {
        on_for_test(true);
        put(change(OWNER, "t-tasks-1"));
        crate::service_records::forgot_about(OWNER, &[vec!["dentists".into()]]);
        assert!(waiting().is_empty());
    }
}
