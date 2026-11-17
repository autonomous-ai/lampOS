//! Reminder rows (`LAMP_SERVICE_RECORDS_REMINDERS`, the linked-service
//! memory plan, slice 6): a reminder a person set by voice, its ring and its
//! cancel are kept in that person's Days memory as linked records, through
//! the linked outbox ([`super::linked`]).
//!
//! - **Whose.** The reminder's creator: the setter the Schedule keeps it
//!   for, as the voice names it beside the call
//!   ([`crate::tools::schedule_whose::caller`]), a known person (`p-` ids)
//!   only. A reminder set as the room's (nobody sure, an unknown face, the
//!   room's line, the private room's "everyone") gives nothing
//!   ([`Kept::NoCreator`]). A ring or a cancel is written for the creator,
//!   never for whoever cancelled it, and only for a reminder the voice saw
//!   set itself: any other id gives nothing ([`Kept::NotOurs`]).
//! - **What.** Key `r-<id>-set`, `r-<id>-fired` or `r-<id>-dismissed`;
//!   service `schedule`, action `reminder_set`, `reminder_fired` or
//!   `reminder_dismissed`, status `done`; `source_kind` `reminder`,
//!   `source_ref` `{service:"lamp-schedule", id:"<id>"}`. The summary is the
//!   reminder's own text as it was set, the field the Schedule already
//!   holds, one line of at most 160 bytes.
//! - **When.** Set: a `set_reminder` or `repeat_reminder` the Schedule
//!   committed. Fired: the second route begins to say it
//!   ([`said`], from the ids the coordinator gives its due request). A
//!   repeating one keeps its first ring's row (the memory keeps a `done`
//!   row as it is). Dismissed: a `cancel_reminder` the Schedule committed.
//!   Timers are not kept.
//! - **Deed time.** Every row of a reminder carries the time it was set, so
//!   a forget after the set wins over a later ring in the memory too.
//! - **Bounded.** At most [`FOLLOWED_MOST`] reminders are followed, oldest
//!   let go first; the map is in memory only, so a reminder set before the
//!   voice restarted gives no ring or cancel row.
//! - **Forgets.** A forget of the person's everything, or about a thing
//!   whose text it matches, lets the reminder go here too, before it is
//!   sent ([`forgot`], [`forgot_about`], called by the linked outbox's own).
//! - **Off.** Unless `LAMP_SERVICE_RECORDS_REMINDERS=on` beside
//!   `LAMP_SERVICE_RECORDS=on`, on the second route: nothing is kept or
//!   followed, and the coordinator gives no ids.
use super::{
    Status,
    linked::{self, Kind, Linked},
    tidy,
};
use serde_json::{Value, json};

/// Only the exact value `on`, beside [`super::ENVIRONMENT`], on the second
/// route.
pub const ENVIRONMENT: &str = "LAMP_SERVICE_RECORDS_REMINDERS";
/// The most reminders followed at once.
pub const FOLLOWED_MOST: usize = 32;
/// The record's service.
pub const SERVICE: &str = "schedule";
/// The service the pointer names.
pub const SOURCE_SERVICE: &str = "lamp-schedule";
/// The tools that set a reminder (not a timer).
const SETS: [&str; 2] = ["set_reminder", "repeat_reminder"];
/// The tool that cancels one.
const CANCELS: &str = "cancel_reminder";

/// What happened to a reminder, a row each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Deed {
    Set,
    Fired,
    Dismissed,
}

impl Deed {
    fn word(self) -> &'static str {
        match self {
            Self::Set => "set",
            Self::Fired => "fired",
            Self::Dismissed => "dismissed",
        }
    }
}

/// One reminder set by a known person and followed.
#[derive(Clone, Debug)]
struct Followed {
    person: String,
    id: u64,
    text: String,
    deed_ms: u64,
}

impl Followed {
    fn change(&self, deed: Deed, changed_ms: u64) -> Linked {
        Linked {
            person: self.person.clone(),
            key: key(self.id, deed),
            kind: Kind::Reminder,
            service: SERVICE.into(),
            action: format!("reminder_{}", deed.word()),
            status: Status::Done,
            source_ref: json!({"service": SOURCE_SERVICE, "id": self.id.to_string()}),
            summary: self.text.clone(),
            deed_ms: self.deed_ms,
            asked_ms: self.deed_ms,
            changed_ms,
            links: Vec::new(),
        }
    }
}

#[cfg(not(test))]
mod kept {
    use super::Followed;
    use std::collections::VecDeque;
    use std::sync::{LazyLock, Mutex};

    static FOLLOWED: Mutex<VecDeque<Followed>> = Mutex::new(VecDeque::new());

    pub fn followed<R>(f: impl FnOnce(&mut VecDeque<Followed>) -> R) -> R {
        f(&mut FOLLOWED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()))
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
    use super::Followed;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;

    thread_local! {
        static FOLLOWED: RefCell<VecDeque<Followed>> = RefCell::default();
        pub static ON: Cell<bool> = const { Cell::new(false) };
    }

    pub fn followed<R>(f: impl FnOnce(&mut VecDeque<Followed>) -> R) -> R {
        FOLLOWED.with(|cell| f(&mut cell.borrow_mut()))
    }

    pub fn on() -> bool {
        ON.with(Cell::get)
    }
}

/// Tests: reminder rows on or off on this thread.
#[cfg(test)]
pub fn on_for_test(on: bool) {
    kept::ON.with(|cell| cell.set(on));
}

/// Whether [`ENVIRONMENT`] is `on`, records or not: the coordinator gives
/// a due request its ids only then, so its packet is as before when off.
pub fn asked() -> bool {
    kept::on()
}

/// Whether reminder rows are kept: [`ENVIRONMENT`] and the records both on.
pub fn on() -> bool {
    kept::on() && super::on()
}

/// A reminder's record key for `deed`.
pub fn key(id: u64, deed: Deed) -> String {
    format!("r-{id}-{}", deed.word())
}

/// What [`after_call`] or [`said`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kept {
    /// A row waits to be written.
    Written(Deed),
    /// A reminder set with no known person as its creator: nothing kept.
    NoCreator,
    /// A ring or cancel of a reminder not followed: nothing kept.
    NotOurs,
    /// Not a reminder the Schedule committed, or not a reminder call.
    Nothing,
    /// Off: nothing kept.
    Off,
}

fn put(followed: &Followed, deed: Deed, now_ms: u64) -> Kept {
    match linked::put(followed.change(deed, now_ms)) {
        linked::Put::Waiting | linked::Put::DroppedOldest => Kept::Written(deed),
        linked::Put::NoPerson => Kept::NoCreator,
        linked::Put::Bad => Kept::Nothing,
        linked::Put::Off => Kept::Off,
    }
}

/// The Schedule's answer `told` to reminder call `name` with `args`, asked
/// with `caller` ([`crate::tools::schedule_whose::caller`]) at `now_ms`: a
/// committed set by a known creator is their row and is followed; a
/// committed cancel of one followed is its creator's dismissed row. Never
/// waits.
pub fn after_call(
    name: &str,
    args: &Value,
    caller: Option<&Value>,
    told: &Value,
    now_ms: u64,
) -> Kept {
    if !on() {
        return Kept::Off;
    }
    let committed = told["status"] == "committed";
    let Some(id) = told["id"].as_u64().filter(|id| committed && *id > 0) else {
        return Kept::Nothing;
    };
    if SETS.contains(&name) {
        if told["cancelled"] == true || told["snoozed"] == true {
            return Kept::Nothing;
        }
        let creator = caller
            .and_then(|caller| caller["setter"].as_str())
            .filter(|person| lamp_msgs::known::is_person_id(person));
        let Some(person) = creator else {
            eprintln!(
                "lamp-voice: reminder {id} set with no known person as its creator: no linked record (LAMP_SERVICE_RECORDS_REMINDERS)"
            );
            return Kept::NoCreator;
        };
        let Some(text) = args["text"].as_str().filter(|text| !text.trim().is_empty()) else {
            return Kept::Nothing;
        };
        let followed = Followed {
            person: person.to_owned(),
            id,
            text: tidy(text, linked::SUMMARY_BYTES),
            deed_ms: now_ms,
        };
        let kept = put(&followed, Deed::Set, now_ms);
        if kept == Kept::Written(Deed::Set) {
            kept::followed(|all| {
                all.retain(|one| one.id != id);
                if all.len() >= FOLLOWED_MOST {
                    all.pop_front();
                }
                all.push_back(followed);
            });
        }
        return kept;
    }
    if name == CANCELS && told["cancelled"] == true {
        let Some(followed) = kept::followed(|all| {
            let at = all.iter().position(|one| one.id == id)?;
            all.remove(at)
        }) else {
            return Kept::NotOurs;
        };
        return put(&followed, Deed::Dismissed, now_ms);
    }
    Kept::Nothing
}

/// The second route began to say the due reminders `ids` at `now_ms`:
/// each one followed is its creator's fired row; any other gives nothing.
pub fn said(ids: &[u64], now_ms: u64) -> Vec<Kept> {
    if !on() {
        return vec![Kept::Off; ids.len()];
    }
    ids.iter()
        .map(|id| {
            let followed = kept::followed(|all| all.iter().find(|one| one.id == *id).cloned());
            match followed {
                Some(followed) => put(&followed, Deed::Fired, now_ms),
                None => Kept::NotOurs,
            }
        })
        .collect()
}

/// A forget of `person`'s everything, or everyone's (`None`): their
/// reminders are let go.
pub fn forgot(person: Option<&str>) {
    kept::followed(|all| all.retain(|one| person.is_some_and(|person| one.person != person)));
}

/// A forget about a thing of `person`'s: their reminders whose text
/// `matches` are let go.
pub fn forgot_about(person: &str, matches: &impl Fn(&str) -> bool) {
    kept::followed(|all| {
        all.retain(|one| {
            one.person != person || !matches(&format!("{SERVICE} reminder_set {}", one.text))
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service_records::on_for_test as records_on;

    const OWNER: &str = "p-0123456789abcdef";
    const OTHER: &str = "p-fedcba9876543210";

    fn on() {
        records_on(true);
        on_for_test(true);
    }

    fn setter(person: Option<&str>) -> Value {
        json!({"asker":{"person":person,"owner":false},"setter":person})
    }

    fn committed(id: u64) -> Value {
        json!({"status":"committed","id":id,"delivery":1,"due_unix_ms":5,"local":"x","clock":"18:00"})
    }

    fn set(person: Option<&str>, id: u64, text: &str) -> Kept {
        after_call(
            "set_reminder",
            &json!({"text":text,"time":"18:00"}),
            Some(&setter(person)),
            &committed(id),
            1_000,
        )
    }

    fn keys() -> Vec<String> {
        linked::waiting()
            .into_iter()
            .map(|change| change.key)
            .collect()
    }

    /// Creator is mapped: a reminder a known person set is their row, the
    /// set's; its ring and its cancel, by whoever, are the creator's.
    #[test]
    fn the_creator_is_mapped() {
        on();
        assert_eq!(set(Some(OWNER), 7, "Call mum"), Kept::Written(Deed::Set));
        assert_eq!(said(&[7], 2_000), [Kept::Written(Deed::Fired)]);
        let cancel = after_call(
            "cancel_reminder",
            &json!({"id":7}),
            Some(&setter(Some(OTHER))),
            &json!({"status":"committed","id":7,"cancelled":true}),
            3_000,
        );
        assert_eq!(cancel, Kept::Written(Deed::Dismissed));
        let waiting = linked::waiting();
        assert_eq!(keys(), ["r-7-set", "r-7-fired", "r-7-dismissed"]);
        for (change, action) in
            waiting
                .iter()
                .zip(["reminder_set", "reminder_fired", "reminder_dismissed"])
        {
            assert_eq!(change.person, OWNER);
            assert_eq!(change.kind, Kind::Reminder);
            assert_eq!(change.service, "schedule");
            assert_eq!(change.action, action);
            assert_eq!(change.status, Status::Done);
            assert_eq!(change.summary, "Call mum");
            assert_eq!(change.deed_ms, 1_000, "every row the set's deed time");
            assert_eq!(
                change.source_ref,
                json!({"service":"lamp-schedule","id":"7"})
            );
        }
        let line = waiting[0].line();
        assert_eq!(line["source_kind"], "reminder");
        // Dismissed, it is no longer followed: a later ring gives nothing.
        assert_eq!(said(&[7], 4_000), [Kept::NotOurs]);
    }

    /// An unknown creator gives nothing: the room's line, nobody sure, an
    /// unknown face, the private room's "everyone", or no caller at all;
    /// and its ring and cancel give nothing either.
    #[test]
    fn an_unknown_creator_gives_nothing() {
        on();
        for person in [
            None,
            Some("room"),
            Some("guest-1"),
            Some("p-short"),
            Some(""),
        ] {
            assert_eq!(
                set(person, 9, "Water the plants"),
                Kept::NoCreator,
                "{person:?}"
            );
        }
        let everyone = after_call(
            "set_reminder",
            &json!({"text":"Water"}),
            Some(&json!({"everyone":true})),
            &committed(9),
            1_000,
        );
        assert_eq!(everyone, Kept::NoCreator);
        let none = after_call(
            "repeat_reminder",
            &json!({"text":"Water"}),
            None,
            &committed(9),
            1_000,
        );
        assert_eq!(none, Kept::NoCreator);
        assert_eq!(said(&[9], 2_000), [Kept::NotOurs]);
        let cancel = after_call(
            "cancel_reminder",
            &json!({"id":9}),
            Some(&setter(Some(OWNER))),
            &json!({"status":"committed","id":9,"cancelled":true}),
            3_000,
        );
        assert_eq!(cancel, Kept::NotOurs);
        assert!(linked::waiting().is_empty());
    }

    /// Off (either switch), nothing is kept or followed.
    #[test]
    fn off_nothing_is_kept() {
        records_on(true);
        on_for_test(false);
        assert_eq!(set(Some(OWNER), 7, "Call mum"), Kept::Off);
        on_for_test(true);
        records_on(false);
        assert_eq!(set(Some(OWNER), 7, "Call mum"), Kept::Off);
        assert_eq!(said(&[7], 2_000), [Kept::Off]);
        records_on(true);
        assert_eq!(said(&[7], 2_000), [Kept::NotOurs]);
        assert!(linked::waiting().is_empty());
    }

    /// A timer, an answer not committed, or a cancel that cancelled
    /// nothing, keeps nothing.
    #[test]
    fn only_a_committed_reminder_is_kept() {
        on();
        let timer = after_call(
            "set_timer",
            &json!({"text":"Tea","seconds":60}),
            Some(&setter(Some(OWNER))),
            &committed(3),
            1_000,
        );
        assert_eq!(timer, Kept::Nothing);
        for told in [
            json!({"error":"I could not confirm what happened to that reminder."}),
            json!({"status":"already_set","id":3}),
            json!({"status":"committed"}),
            json!({"status":"committed","id":0}),
        ] {
            let kept = after_call(
                "set_reminder",
                &json!({"text":"Tea"}),
                Some(&setter(Some(OWNER))),
                &told,
                1_000,
            );
            assert_eq!(kept, Kept::Nothing, "{told}");
        }
        set(Some(OWNER), 4, "Stretch");
        let unsure = after_call(
            "cancel_reminder",
            &json!({"text":"Stretch"}),
            Some(&setter(Some(OWNER))),
            &json!({"cancelled":false,"matches":[]}),
            2_000,
        );
        assert_eq!(unsure, Kept::Nothing);
        assert_eq!(keys(), ["r-4-set"]);
    }

    /// The summary is the reminder's text, one line of at most 160 bytes.
    #[test]
    fn the_summary_is_the_text_in_one_short_line() {
        on();
        set(
            Some(OWNER),
            5,
            &format!("Gọi\nmẹ {}", "lúc sáu giờ ".repeat(30)),
        );
        let summary = &linked::waiting()[0].summary;
        assert!(summary.len() <= linked::SUMMARY_BYTES, "{}", summary.len());
        assert!(summary.starts_with("Gọi mẹ"), "{summary}");
    }

    /// At most [`FOLLOWED_MOST`] are followed, the oldest let go first.
    #[test]
    fn the_followed_are_bounded() {
        on();
        for id in 1..=FOLLOWED_MOST as u64 + 1 {
            set(Some(OWNER), id, "Tea");
        }
        assert_eq!(said(&[1], 2_000), [Kept::NotOurs]);
        assert_eq!(said(&[2], 2_000), [Kept::Written(Deed::Fired)]);
    }

    /// A forget of everything, or about a thing the text matches, lets the
    /// reminder go: no ring row after it; another person's stays.
    #[test]
    fn a_forget_lets_them_go() {
        on();
        set(Some(OWNER), 1, "Call mum");
        set(Some(OWNER), 2, "Take the pills");
        set(Some(OTHER), 3, "Call mum");
        crate::service_records::unsent_about(OWNER, &[vec!["mum".into()]]);
        assert_eq!(keys(), ["r-2-set", "r-3-set"]);
        assert_eq!(said(&[1], 2_000), [Kept::NotOurs]);
        crate::service_records::unsent(OWNER);
        assert_eq!(said(&[2], 2_000), [Kept::NotOurs]);
        assert_eq!(said(&[3], 2_000), [Kept::Written(Deed::Fired)]);
        crate::service_records::forgot(None);
        assert_eq!(said(&[3], 2_000), [Kept::NotOurs]);
    }
}
