//! Harness task rows (`LAMP_SERVICE_RECORDS_TASKS`, the linked-service
//! memory plan, slice 3): work the voice handed over to an agent through
//! `lamp-tasks`, on a person's spoken yes, is kept in that person's Days
//! memory as one linked record, its outcome and a one-line summary, through
//! the linked outbox ([`super::linked`]).
//!
//! - **Whose.** The person whose yes started it, as the services find the
//!   lamp's owner (`services::owner_of`): with no such person, nothing is
//!   kept or followed. The voice's own map from task to person is the only
//!   way to a row: a task the voice did not hand over itself, or one named
//!   on the bus by anyone, is never written ([`Look::NotOurs`]).
//! - **What.** Key `t-<store>-<task>`, service `harness`, action `task`,
//!   `source_kind` `task`, `source_ref` `{service:"lamp-tasks", id:
//!   "<store>:<task>"}`. The summary is the work as it was asked, the field
//!   the tasks part already holds, one line of at most 160 bytes; never the
//!   agent's answer, which stays in `lamp-tasks` (`get_observation`).
//! - **When.** `prepared` at the hand-over; then, from the task's newest
//!   observation, `done` (idle), `failed` (failed, or never sent) or
//!   `cancelled` (stopped). A question back (`needs-input`) or work going on
//!   writes nothing. Each change is written once.
//! - **Deed time.** Every row of a task carries its hand-over time, so a
//!   forget after the hand-over wins over a late outcome in the memory too.
//! - **Bounded.** At most [`FOLLOWED_MOST`] tasks are followed, oldest let
//!   go first, each for [`FOLLOW_FOR_MS`]; one `get_task` a task every
//!   [`LOOK_EVERY`], on a thread of its own, bounded by [`ASK_DEADLINE`].
//!   The map is in memory only: a task settled after the voice restarts
//!   keeps its `prepared` row.
//! - **Forgets.** A forget of the person's everything, or about a thing
//!   whose summary it matches, lets the task go here too, before it is sent
//!   ([`forgot`], [`forgot_about`], called by the linked outbox's own).
//! - **Off.** Unless `LAMP_SERVICE_RECORDS_TASKS=on` beside
//!   `LAMP_SERVICE_RECORDS=on`, on the second route: nothing is kept or
//!   followed, and no `get_task` is asked.
use super::{
    Status,
    linked::{self, Kind, Linked},
    tidy,
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Only the exact value `on`, beside [`super::ENVIRONMENT`], on the second
/// route.
pub const ENVIRONMENT: &str = "LAMP_SERVICE_RECORDS_TASKS";
/// The most tasks followed at once.
pub const FOLLOWED_MOST: usize = 16;
/// How long a task is followed after its hand-over: as long as its news
/// is told (`task_news`, 12 h).
pub const FOLLOW_FOR_MS: u64 = 12 * 3_600_000;
/// How often the tasks followed are looked at.
#[cfg(not(test))]
const LOOK_EVERY: std::time::Duration = std::time::Duration::from_secs(10);
/// The longest one `get_task` waits.
#[cfg(not(test))]
const ASK_DEADLINE: std::time::Duration = std::time::Duration::from_millis(1_500);
/// The record's service and action.
pub const SERVICE: &str = "harness";
pub const ACTION: &str = "task";
/// The service the pointer names.
pub const SOURCE_SERVICE: &str = "lamp-tasks";

/// One task handed over and followed.
#[derive(Clone, Debug)]
struct Followed {
    person: String,
    store: String,
    id: u64,
    summary: String,
    deed_ms: u64,
    /// The tasks part's `request.sock`.
    socket: PathBuf,
    /// The last outcome written ([`outcome`]'s mark).
    written: Option<u64>,
}

impl Followed {
    fn change(&self, status: Status, changed_ms: u64) -> Linked {
        Linked {
            person: self.person.clone(),
            key: key(&self.store, self.id),
            kind: Kind::Task,
            service: SERVICE.into(),
            action: ACTION.into(),
            status,
            source_ref: json!({"service": SOURCE_SERVICE, "id": format!("{}:{}", self.store, self.id)}),
            summary: self.summary.clone(),
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

/// Tests: task rows on or off on this thread.
#[cfg(test)]
pub fn on_for_test(on: bool) {
    kept::ON.with(|cell| cell.set(on));
}

/// Whether task rows are kept: [`ENVIRONMENT`] and the records both on.
pub fn on() -> bool {
    kept::on() && super::on()
}

/// A task's record key.
pub fn key(store: &str, id: u64) -> String {
    format!("t-{store}-{id}")
}

/// What [`handed_over`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handed {
    /// Its row waits to be written, and the task is followed.
    Kept,
    /// No known person said the yes: nothing kept or followed.
    NoPerson,
    /// A store, or a record, the memory would refuse: nothing kept.
    Bad,
    /// Off: nothing kept.
    Off,
}

/// Task `id` of `store`, asked as `asked`, was handed over on `person`'s
/// yes at `now_ms`: its `prepared` row is queued, and it is followed
/// through `socket` for its outcome. Never waits.
pub fn handed_over(
    person: Option<&str>,
    socket: &Path,
    store: &str,
    id: u64,
    asked: &str,
    now_ms: u64,
) -> Handed {
    if !on() {
        return Handed::Off;
    }
    let Some(person) = person.filter(|person| lamp_msgs::known::is_person_id(person)) else {
        eprintln!(
            "lamp-voice: task {id} handed over with no known person's yes: no linked record (LAMP_SERVICE_RECORDS_TASKS)"
        );
        return Handed::NoPerson;
    };
    if store.is_empty() || !store.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Handed::Bad;
    }
    let followed = Followed {
        person: person.to_owned(),
        store: store.to_owned(),
        id,
        summary: tidy(asked, linked::SUMMARY_BYTES),
        deed_ms: now_ms,
        socket: socket.to_path_buf(),
        written: None,
    };
    match linked::put(followed.change(Status::Prepared, now_ms)) {
        linked::Put::Waiting | linked::Put::DroppedOldest => {}
        linked::Put::NoPerson => return Handed::NoPerson,
        linked::Put::Bad => return Handed::Bad,
        linked::Put::Off => return Handed::Off,
    }
    kept::followed(|all| {
        all.retain(|task| !(task.store == followed.store && task.id == followed.id));
        if all.len() >= FOLLOWED_MOST {
            all.pop_front();
        }
        all.push_back(followed);
    });
    start_follower();
    Handed::Kept
}

/// What a task's newest outcome is, from `lamp-tasks`' `get_task` answer:
/// a mark of it (the observation's id plus one, or 0 for a hand-over never
/// sent) and its status. None while work goes on, at a question back, or
/// with an unsure hand-over.
pub fn outcome(task: &Value) -> Option<(u64, Status)> {
    if task["task"]["dispatch"]["state"].as_str() == Some("not_sent") {
        return Some((0, Status::Failed));
    }
    let newest = task["observations"]
        .as_array()?
        .iter()
        .filter_map(|observation| Some((observation["id"].as_u64()?, observation)))
        .max_by_key(|(id, _)| *id)?;
    let status = match newest.1["state"].as_str()? {
        "idle" => Status::Done,
        "failed" => Status::Failed,
        "stopped" => Status::Cancelled,
        _ => return None,
    };
    Some((newest.0.saturating_add(1), status))
}

/// What one look at a task gave.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    /// A new outcome, queued.
    Written(Status),
    /// Nothing new.
    Same,
    /// Not a task the voice handed over on a known person's yes, or one let
    /// go: nothing written.
    NotOurs,
}

/// `answer`, `lamp-tasks`' `get_task` for task `id` of `store`, read at
/// `now_ms`: a new outcome of a task followed is queued once.
pub fn seen(store: &str, id: u64, answer: &Value, now_ms: u64) -> Look {
    if !on() {
        return Look::NotOurs;
    }
    let Some((mark, status)) = outcome(answer) else {
        return if kept::followed(|all| all.iter().any(|task| task.store == store && task.id == id))
        {
            Look::Same
        } else {
            Look::NotOurs
        };
    };
    let change = kept::followed(|all| {
        let task = all
            .iter_mut()
            .find(|task| task.store == store && task.id == id)?;
        if task.written == Some(mark) {
            return Some(None);
        }
        task.written = Some(mark);
        Some(Some(task.change(status, now_ms)))
    });
    match change {
        None => Look::NotOurs,
        Some(None) => Look::Same,
        Some(Some(change)) => {
            linked::put(change);
            Look::Written(status)
        }
    }
}

/// The tasks followed at `now_ms`, `(socket, store, id)` each; those
/// followed longer than [`FOLLOW_FOR_MS`] are let go first.
fn to_look_at(now_ms: u64) -> Vec<(PathBuf, String, u64)> {
    kept::followed(|all| {
        all.retain(|task| now_ms.saturating_sub(task.deed_ms) <= FOLLOW_FOR_MS);
        all.iter()
            .map(|task| (task.socket.clone(), task.store.clone(), task.id))
            .collect()
    })
}

/// One look at every task followed, each asked with `get_task`, the map's
/// lock never held while it waits.
pub fn look(get_task: impl Fn(&Path, u64) -> Option<Value>, now_ms: u64) {
    for (socket, store, id) in to_look_at(now_ms) {
        if let Some(answer) = get_task(&socket, id) {
            seen(&store, id, &answer, now_ms);
        }
    }
}

/// The tasks followed, `(key, person)` each, oldest first.
pub fn followed() -> Vec<(String, String)> {
    kept::followed(|all| {
        all.iter()
            .map(|task| (key(&task.store, task.id), task.person.clone()))
            .collect()
    })
}

/// A forget of `person`'s everything, or everyone's (`None`): their tasks
/// are no longer followed.
pub fn forgot(person: Option<&str>) {
    kept::followed(|all| all.retain(|task| person.is_some_and(|person| task.person != person)));
}

/// A forget about a thing of `person`'s: their tasks whose service, action
/// or summary `matches` are no longer followed.
pub fn forgot_about(person: &str, matches: &impl Fn(&str) -> bool) {
    kept::followed(|all| {
        all.retain(|task| {
            task.person != person || !matches(&format!("{SERVICE} {ACTION} {}", task.summary))
        });
    });
}

/// The follower: a thread of its own, started at the first hand-over,
/// looking every [`LOOK_EVERY`] while any task is followed.
#[cfg(not(test))]
fn start_follower() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        let spawned = std::thread::Builder::new()
            .name("linked-task-rows".into())
            .spawn(|| {
                loop {
                    std::thread::sleep(LOOK_EVERY);
                    look(
                        |socket, id| {
                            crate::agent_tasks::ask(
                                socket,
                                &json!({"op":"get_task","id":id}),
                                ASK_DEADLINE,
                            )
                            .filter(|answer| answer.get("ok") != Some(&json!(false)))
                        },
                        super::unix_ms(),
                    );
                }
            });
        if spawned.is_err() {
            eprintln!(
                "lamp-voice: the task rows follower did not start; task outcomes are not saved (LAMP_SERVICE_RECORDS_TASKS)"
            );
        }
    });
}

#[cfg(test)]
fn start_follower() {}

/// Tests: the outcome a task's `get_task` gives, with `observations` its
/// `(id, state)`s and `dispatch` its state.
#[cfg(test)]
pub(crate) fn task_answer(dispatch: &str, observations: &[(u64, &str)]) -> Value {
    json!({
        "task": {"dispatch": {"state": dispatch}},
        "text": "asked",
        "observations": observations
            .iter()
            .map(|(id, state)| json!({"id": id, "state": state}))
            .collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service_records::on_for_test as records_on;

    const OWNER: &str = "p-0123456789abcdef";
    const OTHER: &str = "p-fedcba9876543210";
    const STORE: &str = "0123456789abcdef0123456789abcdef";

    fn both_on() {
        records_on(true);
        on_for_test(true);
    }

    fn hand(person: &str, id: u64, asked: &str) -> Handed {
        handed_over(
            Some(person),
            Path::new("/run/lamp-tasks/request.sock"),
            STORE,
            id,
            asked,
            1_000,
        )
    }

    /// Off, either switch, nothing is kept or followed.
    #[test]
    fn off_nothing_is_kept_or_followed() {
        for (records, rows) in [(false, false), (true, false), (false, true)] {
            records_on(records);
            on_for_test(rows);
            assert_eq!(hand(OWNER, 7, "Book the dentist"), Handed::Off);
            assert!(linked::waiting().is_empty());
            assert!(followed().is_empty());
            assert_eq!(
                seen(STORE, 7, &task_answer("accepted", &[(1, "idle")]), 2_000),
                Look::NotOurs
            );
        }
    }

    /// The yes-person is mapped: the hand-over's row is theirs, `prepared`,
    /// with its pointer and the work as asked, and the task is followed.
    #[test]
    fn the_yes_person_is_mapped() {
        both_on();
        assert_eq!(hand(OWNER, 7, "Book the dentist for Tuesday"), Handed::Kept);
        let waiting = linked::waiting();
        assert_eq!(waiting.len(), 1);
        let row = &waiting[0];
        assert_eq!(row.person, OWNER);
        assert_eq!(row.key, format!("t-{STORE}-7"));
        assert_eq!(row.kind, Kind::Task);
        assert_eq!(row.status, Status::Prepared);
        assert_eq!(row.summary, "Book the dentist for Tuesday");
        assert_eq!(row.deed_ms, 1_000);
        let line = row.line();
        assert_eq!(line["source_kind"], "task");
        assert_eq!(line["source_ref"]["service"], "lamp-tasks");
        assert_eq!(line["source_ref"]["id"], format!("{STORE}:7"));
        assert_eq!(followed(), [(format!("t-{STORE}-7"), OWNER.to_owned())]);
    }

    /// With no known person's yes, nothing is kept or followed.
    #[test]
    fn no_known_person_keeps_nothing() {
        both_on();
        let socket = Path::new("/run/lamp-tasks/request.sock");
        assert_eq!(
            handed_over(None, socket, STORE, 7, "Book it", 1_000),
            Handed::NoPerson
        );
        for person in ["", "the-owner", "room", "p-short"] {
            assert_eq!(hand(person, 7, "Book it"), Handed::NoPerson, "{person}");
        }
        assert!(linked::waiting().is_empty());
        assert!(followed().is_empty());
    }

    /// A store the key can't hold is refused.
    #[test]
    fn a_bad_store_keeps_nothing() {
        both_on();
        let socket = Path::new("/run/lamp-tasks/request.sock");
        for store in ["", "s-1", "a b", &"s".repeat(70)] {
            assert_eq!(
                handed_over(Some(OWNER), socket, store, 7, "Book it", 1_000),
                Handed::Bad,
                "{store}"
            );
        }
        assert!(followed().is_empty());
    }

    /// The summary is one line of at most 160 bytes, cut at a whole
    /// character.
    #[test]
    fn the_summary_is_at_most_160_bytes() {
        both_on();
        hand(
            OWNER,
            7,
            &format!("Viết\nbáo cáo {}", "về doanh thu ".repeat(30)),
        );
        let summary = &linked::waiting()[0].summary;
        assert!(summary.len() <= linked::SUMMARY_BYTES, "{}", summary.len());
        assert!(summary.starts_with("Viết báo cáo"), "{summary}");
        assert!(!summary.contains('\n'));
    }

    /// An unknown or forged task, settled, gives nothing: only a task the
    /// voice handed over is written.
    #[test]
    fn an_unknown_or_forged_task_is_dropped() {
        both_on();
        hand(OWNER, 7, "Book the dentist");
        let before = linked::waiting().len();
        let settled = task_answer("accepted", &[(1, "idle")]);
        assert_eq!(seen(STORE, 8, &settled, 2_000), Look::NotOurs);
        assert_eq!(
            seen("ffffffffffffffffffffffffffffffff", 7, &settled, 2_000),
            Look::NotOurs
        );
        assert_eq!(linked::waiting().len(), before);
    }

    /// A settled task's outcome is written once, for its person, with the
    /// hand-over's deed time; a later outcome is written again.
    #[test]
    fn an_outcome_is_written_once_with_the_hand_over_deed() {
        both_on();
        hand(OWNER, 7, "Book the dentist");
        let working = task_answer("accepted", &[(1, "working")]);
        assert_eq!(seen(STORE, 7, &working, 2_000), Look::Same);
        let done = task_answer("accepted", &[(1, "working"), (2, "idle")]);
        assert_eq!(seen(STORE, 7, &done, 3_000), Look::Written(Status::Done));
        assert_eq!(seen(STORE, 7, &done, 4_000), Look::Same);
        let row = linked::waiting().pop().unwrap();
        assert_eq!(row.status, Status::Done);
        assert_eq!(row.person, OWNER);
        assert_eq!(row.deed_ms, 1_000);
        assert_eq!(row.changed_ms, 3_000);
        // The prepared row was replaced by the done one: one key, latest.
        assert_eq!(linked::waiting().len(), 1);
        let failed = task_answer("accepted", &[(2, "idle"), (3, "failed")]);
        assert_eq!(
            seen(STORE, 7, &failed, 5_000),
            Look::Written(Status::Failed)
        );
    }

    /// The outcomes: idle done, failed failed, stopped cancelled, never
    /// sent failed; a question back, work going on, or an unsure hand-over
    /// nothing.
    #[test]
    fn outcomes_from_the_task() {
        assert_eq!(
            outcome(&task_answer("accepted", &[(4, "idle")])),
            Some((5, Status::Done))
        );
        assert_eq!(
            outcome(&task_answer("accepted", &[(4, "failed")])),
            Some((5, Status::Failed))
        );
        assert_eq!(
            outcome(&task_answer("accepted", &[(4, "stopped")])),
            Some((5, Status::Cancelled))
        );
        assert_eq!(
            outcome(&task_answer("not_sent", &[])),
            Some((0, Status::Failed))
        );
        assert_eq!(
            outcome(&task_answer("accepted", &[(4, "needs-input")])),
            None
        );
        assert_eq!(
            outcome(&task_answer("accepted", &[(4, "idle"), (5, "working")])),
            None
        );
        assert_eq!(outcome(&task_answer("uncertain", &[])), None);
        assert_eq!(outcome(&json!({})), None);
    }

    /// A forget of the person's everything lets their tasks go, so a later
    /// outcome writes nothing; another person's stay.
    #[test]
    fn a_forget_lets_their_tasks_go() {
        both_on();
        hand(OWNER, 7, "Book the dentist");
        hand(OTHER, 8, "Water the plants");
        crate::service_records::unsent(OWNER);
        assert_eq!(followed(), [(format!("t-{STORE}-8"), OTHER.to_owned())]);
        let done = task_answer("accepted", &[(1, "idle")]);
        assert_eq!(seen(STORE, 7, &done, 2_000), Look::NotOurs);
        assert!(linked::waiting().iter().all(|row| row.person == OTHER));
        crate::service_records::forgot(None);
        assert!(followed().is_empty());
    }

    /// A forget about a thing lets only the tasks it matches go.
    #[test]
    fn a_forget_about_lets_only_the_matching_tasks_go() {
        both_on();
        hand(OWNER, 7, "Book the dentist");
        hand(OWNER, 8, "Water the plants");
        crate::service_records::unsent_about(OWNER, &[vec!["dentist".into()]]);
        assert_eq!(followed(), [(format!("t-{STORE}-8"), OWNER.to_owned())]);
    }

    /// At most 16 tasks are followed, the oldest let go first; one followed
    /// past 12 h is let go at the next look, unasked.
    #[test]
    fn followed_is_bounded_in_number_and_time() {
        both_on();
        for id in 0..FOLLOWED_MOST as u64 + 2 {
            hand(OWNER, id, "Do it");
        }
        let keys: Vec<String> = followed().into_iter().map(|(key, _)| key).collect();
        assert_eq!(keys.len(), FOLLOWED_MOST);
        assert_eq!(keys[0], format!("t-{STORE}-2"));
        let asked = std::cell::Cell::new(0);
        look(
            |_, _| {
                asked.set(asked.get() + 1);
                None
            },
            1_000 + FOLLOW_FOR_MS,
        );
        assert_eq!(asked.get(), FOLLOWED_MOST);
        look(|_, _| panic!("let go: never asked"), 1_001 + FOLLOW_FOR_MS);
        assert!(followed().is_empty());
    }

    /// A look asks each task followed at its own socket and writes what
    /// settled.
    #[test]
    fn a_look_writes_what_settled() {
        both_on();
        hand(OWNER, 7, "Book the dentist");
        look(
            |socket, id| {
                assert_eq!(socket, Path::new("/run/lamp-tasks/request.sock"));
                assert_eq!(id, 7);
                Some(task_answer("accepted", &[(1, "idle")]))
            },
            2_000,
        );
        let waiting = linked::waiting();
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].status, Status::Done);
    }
}
