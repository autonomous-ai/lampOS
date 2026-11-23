//! Agent rows (`LAMP_SERVICE_RECORDS_AGENT`, the linked-service memory
//! plan, slice 5): the calls a skill's run made on the lamp's agent, read
//! or write, kept in the Days memory of the person the voice can name for
//! that run, one linked record a call, through the linked outbox
//! ([`super::linked`]).
//!
//! - **Where the calls come from.** `lamp-services --run-calls` keeps a
//!   run's last 64 calls without content (slice 4); the voice pulls them
//!   with `run_calls_since` (its own call on its tools socket, never
//!   offered to its model) every [`PULL_EVERY`], bounded by
//!   [`PULL_DEADLINE`], on a thread of its own, only while something waits
//!   to be named ([`wanted`]). It keeps the last [`CALLS_KEPT`] pulled, no
//!   content either, so a run's earlier reads are still there when it is
//!   named.
//! - **Whose: the run is mapped to the person of its held write.** A run's
//!   write is held by `lamp-services` and read back by the voice to the
//!   services' owner (`services::wait_for_run_yes`); that read-back, a
//!   known person's (`p-` id), names the run: the held write of the same
//!   skill and catalog code in the calls, held before the read-back and
//!   within its two minutes, not named before ([`read_back`]). From then
//!   every call of that run, before and after, is that person's. A run
//!   with no read-back to a known person (a read-only run, a write never
//!   read back, or calls the voice never saw held) is dropped, counted and
//!   said in one journal line, its run id only ([`unknown`]).
//! - **What.** Key `a-<run>-<n>` (`n` the call's number in the ring),
//!   service its catalog code, action the call's own word, `source_kind`
//!   `agent`, `source_ref` `{service:"lamp-services", id:"<run>:<n>",
//!   session:"<run>"}` (the run stays in the agent: a pointer only). The
//!   summary is built from those fields alone ("Your mail-count skill:
//!   gmail send_mail"), never the call's content, which `lamp-services`
//!   never kept. A read is `done`, a refused call `refused`, a skill's
//!   fixed write sent `done`.
//! - **A held write gives one row**, its key the held call's: `prepared`
//!   from the read-back, then the yes's outcome under the same key, as the
//!   send settles ([`settled`], [`sent`]): `done` sent, `cancelled` a no or
//!   a replaced write, `refused` not the owner's plain yes, `failed` not
//!   sent.
//! - **Deed time.** Each row carries its call's time, so a forget after
//!   the call wins over a late write in the memory too.
//! - **Bounded, in memory only.** At most [`READ_BACKS_MOST`] read-backs
//!   wait to be matched, each for [`MATCH_FOR_MS`]; at most [`RUNS_MOST`]
//!   runs are named (lamp-control's own bound on runs at once), each for
//!   [`RUN_FOR_MS`] (its longest token); the oldest go first.
//! - **Forgets.** A forget of the person's everything, or about a thing
//!   whose summary it matches, lets their read-backs, runs and calls go
//!   here too, before it is sent ([`forgot`], [`forgot_about`], called by
//!   the linked outbox's own).
//! - **Off.** Unless `LAMP_SERVICE_RECORDS_AGENT=on` beside
//!   `LAMP_SERVICE_RECORDS=on`, on the second route: nothing is kept, and
//!   `run_calls_since` is never asked.
use super::{
    Got, Status,
    linked::{self, Kind, Linked},
};
use serde_json::{Value, json};
use std::collections::VecDeque;

/// Only the exact value `on`, beside [`super::ENVIRONMENT`], on the second
/// route.
pub const ENVIRONMENT: &str = "LAMP_SERVICE_RECORDS_AGENT";
/// The most read-backs waiting for their run.
pub const READ_BACKS_MOST: usize = 8;
/// How long a read-back waits for its held call: its two minutes, and a
/// pull's worth more.
pub const MATCH_FOR_MS: u64 = 3 * 60_000;
/// How long before a read-back its held call may be: the hold's two
/// minutes.
pub const HELD_BEFORE_MS: u64 = 2 * 60_000;
/// The most runs named at once (lamp-control's `runs::MOST_RUNS`).
pub const RUNS_MOST: usize = 8;
/// How long a run stays named: lamp-control's longest token
/// (`runs::LONGEST_MS`).
pub const RUN_FOR_MS: u64 = 30 * 60_000;
/// The most calls kept from the pulls (the ring's own 64).
pub const CALLS_KEPT: usize = 64;
/// The service a row's pointer names.
pub const SOURCE_SERVICE: &str = "lamp-services";
/// How often the calls are pulled, while something waits.
#[cfg(not(test))]
const PULL_EVERY: std::time::Duration = std::time::Duration::from_secs(5);
/// The longest one pull waits.
#[cfg(not(test))]
const PULL_DEADLINE: std::time::Duration = std::time::Duration::from_secs(1);
/// The most pulls in a row while `lamp-services` says more wait.
#[cfg(not(test))]
const PULLS_IN_A_ROW: usize = 4;

/// One call of a skill's run, as pulled: no content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    pub n: u64,
    pub run: String,
    pub skill: String,
    /// `service_read` or `service_write`.
    pub tool: String,
    pub service: String,
    pub action: String,
    /// `read`, `held`, `sent` or `refused`.
    pub status: String,
    pub at_ms: u64,
}

impl Call {
    /// From one of `run_calls_since`'s `calls`: none for a call naming no
    /// catalog code, or not as `lamp-services` gives it.
    pub fn of(call: &Value) -> Option<Self> {
        let text = |key: &str| call[key].as_str().map(str::to_owned);
        let call = Self {
            n: call["n"].as_u64()?,
            run: text("run")?,
            skill: text("skill")?,
            tool: text("tool")?,
            service: text("service")?,
            action: text("action")?,
            status: text("status")?,
            at_ms: call["at_unix_ms"].as_u64()?,
        };
        let plain = |text: &str| {
            (1..=64).contains(&text.len())
                && text
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        };
        (plain(&call.run)
            && plain(&call.skill)
            && plain(&call.service)
            && plain(&call.action)
            && matches!(call.tool.as_str(), "service_read" | "service_write"))
        .then_some(call)
    }

    fn held_write(&self) -> bool {
        self.tool == "service_write" && self.status == "held"
    }

    fn key(&self) -> String {
        format!("a-{}-{}", self.run, self.n)
    }

    fn summary(&self) -> String {
        format!(
            "Your {} skill: {} {}",
            self.skill, self.service, self.action
        )
    }
}

/// A run's write read back to a person, waiting for its held call.
#[derive(Clone, Debug)]
struct ReadBack {
    person: String,
    skill: String,
    service: String,
    send_id: String,
    at_ms: u64,
    /// The yes's outcome, once settled, and when.
    outcome: Option<(Status, u64)>,
    /// The held call it was matched to.
    matched: Option<Call>,
    /// The status its row was last queued with.
    written: Option<Status>,
}

/// A run named for a person.
#[derive(Clone, Debug)]
struct Named {
    run: String,
    person: String,
    at_ms: u64,
}

#[derive(Default)]
struct Kept {
    read_backs: VecDeque<ReadBack>,
    runs: VecDeque<Named>,
    /// The calls pulled, not yet written or let go, oldest first.
    calls: VecDeque<Call>,
    /// The newest call number pulled, for the next pull.
    after: u64,
    /// Runs let go unnamed, since start.
    unknown: u64,
}

#[cfg(not(test))]
mod kept {
    use super::Kept;
    use std::sync::{LazyLock, Mutex};

    static KEPT: LazyLock<Mutex<Kept>> = LazyLock::new(Mutex::default);

    pub fn kept<R>(f: impl FnOnce(&mut Kept) -> R) -> R {
        f(&mut KEPT.lock().unwrap_or_else(|poisoned| poisoned.into_inner()))
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
    use super::Kept;
    use std::cell::{Cell, RefCell};

    thread_local! {
        static KEPT: RefCell<Kept> = RefCell::default();
        pub static ON: Cell<bool> = const { Cell::new(false) };
    }

    pub fn kept<R>(f: impl FnOnce(&mut Kept) -> R) -> R {
        KEPT.with(|cell| f(&mut cell.borrow_mut()))
    }

    pub fn on() -> bool {
        ON.with(Cell::get)
    }
}

/// Tests: agent rows on or off on this thread.
#[cfg(test)]
pub fn on_for_test(on: bool) {
    kept::ON.with(|cell| cell.set(on));
}

/// Whether agent rows are kept: [`ENVIRONMENT`] and the records both on.
pub fn on() -> bool {
    kept::on() && super::on()
}

/// What [`read_back`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadBackKept {
    /// It waits for its held call.
    Kept,
    /// Not a known person's: nothing kept.
    NoPerson,
    /// Off: nothing kept.
    Off,
}

/// A skill run's write of `skill` to `service` (its catalog code), held
/// as `send_id`, was read back to `person` at `now_ms`: it waits to name
/// its run. Never waits.
pub fn read_back(
    person: &str,
    skill: &str,
    service: &str,
    send_id: &str,
    now_ms: u64,
) -> ReadBackKept {
    if !on() {
        return ReadBackKept::Off;
    }
    if !lamp_msgs::known::is_person_id(person) {
        eprintln!(
            "lamp-voice: a skill run's write read back to no known person: its run is not named (LAMP_SERVICE_RECORDS_AGENT)"
        );
        return ReadBackKept::NoPerson;
    }
    kept::kept(|kept| {
        kept.read_backs.retain(|waiting| waiting.send_id != send_id);
        if kept.read_backs.len() >= READ_BACKS_MOST {
            kept.read_backs.pop_front();
        }
        kept.read_backs.push_back(ReadBack {
            person: person.to_owned(),
            skill: skill.to_owned(),
            service: service.to_owned(),
            send_id: send_id.to_owned(),
            at_ms: now_ms,
            outcome: None,
            matched: None,
            written: None,
        });
    });
    let changes = kept::kept(|kept| kept.settle_all(now_ms));
    put_all(changes);
    start_puller();
    ReadBackKept::Kept
}

/// The read-back write `send_id` was answered `status` at `now_ms`: its
/// row, once its run is named, has that outcome, under its held call's key.
/// The first outcome stands.
pub fn settled(send_id: &str, status: Status, now_ms: u64) {
    if !on() || send_id.is_empty() {
        return;
    }
    let changes = kept::kept(|kept| {
        let Some(waiting) = kept
            .read_backs
            .iter_mut()
            .find(|waiting| waiting.send_id == send_id)
        else {
            return Vec::new();
        };
        // The first outcome is the yes's: a later settle of the same write
        // (its record's own bookkeeping) changes nothing.
        if waiting.outcome.is_none() {
            waiting.outcome = Some((status, now_ms));
        }
        kept.settle_all(now_ms)
    });
    put_all(changes);
}

/// A send try of `send_id` got as far as `got` ([`super::keep_send`]): its
/// outcome, once answered, is [`settled`].
pub fn sent(send_id: &str, got: &Got<'_>, now_ms: u64) {
    let status = match got {
        Got::Asked | Got::Refused(_) => return,
        Got::Answered(told) if crate::services::went(told) => Status::Done,
        Got::Answered(told) if told["cancelled"] == true => Status::Cancelled,
        Got::Answered(_) | Got::Unanswered => Status::Failed,
    };
    settled(send_id, status, now_ms);
}

/// `answer`, `run_calls_since`'s `result`, pulled at `now_ms`: its calls
/// kept, the runs named, and every call of a named run queued once.
pub fn pulled(answer: &Value, now_ms: u64) {
    if !on() {
        return;
    }
    let changes = kept::kept(|kept| {
        // Pulled before, unless `lamp-services` started again.
        let before = if answer["restarted"] == true {
            0
        } else {
            kept.after
        };
        if let Some(last) = answer["last"].as_u64() {
            kept.after = last;
        }
        for call in answer["calls"].as_array().into_iter().flatten() {
            let Some(call) = Call::of(call) else {
                continue;
            };
            if call.n <= before || kept.calls.iter().any(|kept| kept.key() == call.key()) {
                continue;
            }
            if kept.calls.len() >= CALLS_KEPT {
                let gone = kept.calls.pop_front();
                kept.let_go(gone);
            }
            kept.calls.push_back(call);
        }
        kept.settle_all(now_ms)
    });
    put_all(changes);
}

/// The `after` the next pull asks from.
pub fn after() -> u64 {
    kept::kept(|kept| kept.after)
}

/// Whether a pull is wanted at `now_ms`: a read-back waits for its run,
/// or a named run may still call.
pub fn wanted(now_ms: u64) -> bool {
    on() && kept::kept(|kept| {
        kept.expire(now_ms);
        kept.read_backs
            .iter()
            .any(|waiting| waiting.matched.is_none())
            || !kept.runs.is_empty()
    })
}

/// The runs named, `(run, person)` each, oldest first.
pub fn named() -> Vec<(String, String)> {
    kept::kept(|kept| {
        kept.runs
            .iter()
            .map(|named| (named.run.clone(), named.person.clone()))
            .collect()
    })
}

/// How many runs' calls were let go unnamed, since start.
pub fn unknown() -> u64 {
    kept::kept(|kept| kept.unknown)
}

/// A forget of `person`'s everything, or everyone's (`None`): their
/// read-backs, runs and calls go.
pub fn forgot(person: Option<&str>) {
    let theirs = |who: &str| person.is_none_or(|person| person == who);
    kept::kept(|kept| {
        let gone: Vec<String> = kept
            .runs
            .iter()
            .filter(|named| theirs(&named.person))
            .map(|named| named.run.clone())
            .collect();
        kept.runs.retain(|named| !theirs(&named.person));
        kept.read_backs.retain(|waiting| !theirs(&waiting.person));
        kept.calls.retain(|call| !gone.contains(&call.run));
    });
}

/// A forget about a thing of `person`'s: their runs with a call (or a
/// held write read back) whose service, action or summary `matches` go,
/// with their calls and read-backs; and their read-backs not yet matched
/// whose skill or service matches.
pub fn forgot_about(person: &str, matches: &impl Fn(&str) -> bool) {
    let text = |call: &Call| format!("{} {} {}", call.service, call.action, call.summary());
    kept::kept(|kept| {
        let gone: Vec<String> = kept
            .runs
            .iter()
            .filter(|named| named.person == person)
            .filter(|named| {
                kept.calls
                    .iter()
                    .chain(
                        kept.read_backs
                            .iter()
                            .filter_map(|waiting| waiting.matched.as_ref()),
                    )
                    .any(|call| call.run == named.run && matches(&text(call)))
            })
            .map(|named| named.run.clone())
            .collect();
        kept.runs.retain(|named| !gone.contains(&named.run));
        kept.calls.retain(|call| !gone.contains(&call.run));
        kept.read_backs.retain(|waiting| match &waiting.matched {
            Some(call) => !gone.contains(&call.run),
            None => {
                waiting.person != person
                    || !matches(&format!("{} Your {} skill", waiting.service, waiting.skill))
            }
        });
    });
}

impl Kept {
    /// Read-backs, runs and calls past their time go: a read-back not
    /// matched in [`MATCH_FOR_MS`] (one matched stays for its outcome as
    /// long as its run), a run named [`RUN_FOR_MS`] ago, and a call older
    /// than a run lasts.
    fn expire(&mut self, now_ms: u64) {
        self.runs
            .retain(|named| now_ms.saturating_sub(named.at_ms) <= RUN_FOR_MS);
        let runs = &self.runs;
        self.read_backs.retain(|waiting| match &waiting.matched {
            None => now_ms.saturating_sub(waiting.at_ms) <= MATCH_FOR_MS,
            Some(call) => runs.iter().any(|named| named.run == call.run),
        });
        while self
            .calls
            .front()
            .is_some_and(|call| now_ms.saturating_sub(call.at_ms) > RUN_FOR_MS)
        {
            let gone = self.calls.pop_front();
            self.let_go(gone);
        }
    }

    /// A call let go: one of a run never named is counted and said.
    fn let_go(&mut self, call: Option<Call>) {
        let Some(call) = call else {
            return;
        };
        if self.runs.iter().any(|named| named.run == call.run) {
            return;
        }
        self.unknown += 1;
        eprintln!(
            "lamp-voice: a skill run's call ({}) let go: no known person's read-back named run {}; unknown={} (LAMP_SERVICE_RECORDS_AGENT)",
            call.key(),
            call.run,
            self.unknown
        );
    }

    /// Each read-back waiting matched to its held call, naming its run;
    /// then every other call of a named run made a row, once, and each
    /// matched held write's row given its latest outcome, once each. The
    /// changes to queue.
    fn settle_all(&mut self, now_ms: u64) -> Vec<Linked> {
        self.expire(now_ms);
        let mut changes = Vec::new();
        // Name runs: the oldest read-back first, to the newest held call
        // of its skill and code before it, within its hold.
        for at in 0..self.read_backs.len() {
            if self.read_backs[at].matched.is_some() {
                continue;
            }
            let waiting = &self.read_backs[at];
            let Some(found) = self
                .calls
                .iter()
                .filter(|call| {
                    call.held_write()
                        && call.skill == waiting.skill
                        && call.service == waiting.service
                        && call.at_ms <= waiting.at_ms
                        && waiting.at_ms - call.at_ms <= HELD_BEFORE_MS
                })
                .max_by_key(|call| call.n)
                .cloned()
            else {
                continue;
            };
            // The held call is the read-back's from now on.
            self.calls.retain(|call| *call != found);
            let person = waiting.person.clone();
            let run = found.run.clone();
            self.read_backs[at].matched = Some(found);
            if !self.runs.iter().any(|named| named.run == run) {
                if self.runs.len() >= RUNS_MOST {
                    self.runs.pop_front();
                }
                eprintln!(
                    "lamp-voice: skill run {run} named by its write's read-back (LAMP_SERVICE_RECORDS_AGENT)"
                );
                self.runs.push_back(Named {
                    run,
                    person,
                    at_ms: now_ms,
                });
            }
        }
        // Each matched held write: its row when its outcome changed.
        for waiting in &mut self.read_backs {
            let Some(call) = &waiting.matched else {
                continue;
            };
            let (status, changed_ms) = waiting.outcome.unwrap_or((Status::Prepared, waiting.at_ms));
            if waiting.written == Some(status) {
                continue;
            }
            waiting.written = Some(status);
            changes.push(row(call, &waiting.person, status, changed_ms));
        }
        // Every other call of a named run: one row; written, it goes. A
        // held write of a named run never read back stays unwritten: no
        // one heard it, so it is no one's deed yet.
        let mut written = Vec::new();
        for call in &self.calls {
            let Some(named) = self.runs.iter().find(|named| named.run == call.run) else {
                continue;
            };
            let status = match call.status.as_str() {
                "read" | "sent" => Status::Done,
                "refused" => Status::Refused,
                _ => continue,
            };
            changes.push(row(call, &named.person, status, now_ms));
            written.push(call.key());
        }
        self.calls.retain(|call| !written.contains(&call.key()));
        // In the order the calls were made.
        changes.sort_by_key(|change| change.deed_ms);
        changes
    }
}

/// `call`'s row for `person`, `status` as of `changed_ms`.
fn row(call: &Call, person: &str, status: Status, changed_ms: u64) -> Linked {
    Linked {
        person: person.to_owned(),
        key: call.key(),
        kind: Kind::Agent,
        service: call.service.clone(),
        action: call.action.clone(),
        status,
        source_ref: json!({
            "service": SOURCE_SERVICE,
            "id": format!("{}:{}", call.run, call.n),
            "session": call.run,
        }),
        summary: call.summary(),
        deed_ms: call.at_ms,
        asked_ms: call.at_ms,
        changed_ms: changed_ms.max(call.at_ms),
        links: Vec::new(),
    }
}

/// Queues each change, the lock not held.
fn put_all(changes: Vec<Linked>) {
    for change in changes {
        linked::put(change);
    }
}

/// The puller: a thread of its own, started at the first read-back,
/// pulling every [`PULL_EVERY`] while one is [`wanted`].
#[cfg(not(test))]
fn start_puller() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        let spawned = std::thread::Builder::new()
            .name("linked-agent-rows".into())
            .spawn(|| {
                let socket =
                    std::path::Path::new(crate::services::FOLDER).join(crate::tools::CALL_SOCKET);
                loop {
                    std::thread::sleep(PULL_EVERY);
                    for _ in 0..PULLS_IN_A_ROW {
                        if !wanted(super::unix_ms()) {
                            break;
                        }
                        let Some(told) = crate::tools::exchange(
                            &socket,
                            &json!({"op": "call", "name": lamp_services::run_calls::CALL,
                                "args": {"after": after()}}),
                            PULL_DEADLINE,
                        ) else {
                            break;
                        };
                        if told["ok"] != true {
                            break;
                        }
                        pulled(&told["result"], super::unix_ms());
                        if told["result"]["more"] != true {
                            break;
                        }
                    }
                }
            });
        if spawned.is_err() {
            eprintln!(
                "lamp-voice: the agent rows puller did not start; agent rows are not saved (LAMP_SERVICE_RECORDS_AGENT)"
            );
        }
    });
}

#[cfg(test)]
fn start_puller() {}

/// Tests: one call as `run_calls_since` gives it.
#[cfg(test)]
pub(crate) fn call_json(
    n: u64,
    run: &str,
    tool: &str,
    action: &str,
    status: &str,
    at: u64,
) -> Value {
    json!({"n": n, "run": run, "skill": "mail-count", "tool": tool, "service": "gmail",
        "action": action, "status": status, "at_unix_ms": at})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service_records::on_for_test as records_on;

    const OWNER: &str = "p-0123456789abcdef";
    const OTHER: &str = "p-fedcba9876543210";
    const RUN: &str = "task-1790000000000-42-0";
    const ELSE: &str = "task-1790000000000-42-1";

    fn both_on() {
        records_on(true);
        on_for_test(true);
    }

    fn pull(calls: &[Value]) {
        let last = calls
            .iter()
            .filter_map(|call| call["n"].as_u64())
            .max()
            .unwrap_or(0);
        pulled(
            &json!({"calls": calls, "last": last, "more": false}),
            20_000,
        );
    }

    /// A run's read, then its write held.
    fn a_run(run: &str, first: u64) -> Vec<Value> {
        vec![
            call_json(first, run, "service_read", "search", "read", 10_000),
            call_json(first + 1, run, "service_write", "send_mail", "held", 11_000),
        ]
    }

    fn keys() -> Vec<String> {
        linked::waiting().into_iter().map(|row| row.key).collect()
    }

    /// Off, either switch: nothing is kept, named or wanted.
    #[test]
    fn off_nothing_is_kept() {
        for (records, rows) in [(false, false), (true, false), (false, true)] {
            records_on(records);
            on_for_test(rows);
            assert_eq!(
                read_back(OWNER, "mail-count", "gmail", "s1", 12_000),
                ReadBackKept::Off
            );
            pull(&a_run(RUN, 1));
            settled("s1", Status::Done, 13_000);
            assert!(linked::waiting().is_empty());
            assert!(named().is_empty());
            assert!(!wanted(13_000));
        }
    }

    /// Run is mapped to person: the read-back of its held write names the
    /// run for that person, and each of its calls is their row, its read
    /// `done` and its write `prepared`, with the call's deed time and a
    /// pointer to the run, never its content.
    #[test]
    fn the_run_is_mapped_to_the_person() {
        both_on();
        pull(&a_run(RUN, 1));
        assert!(linked::waiting().is_empty(), "nothing before it is named");
        assert_eq!(
            read_back(OWNER, "mail-count", "gmail", "s1", 12_000),
            ReadBackKept::Kept
        );
        assert_eq!(named(), [(RUN.to_owned(), OWNER.to_owned())]);
        let rows = linked::waiting();
        assert_eq!(rows.len(), 2, "{rows:?}");
        let (read, write) = (&rows[0], &rows[1]);
        assert!(
            rows.iter()
                .all(|row| row.person == OWNER && row.kind == Kind::Agent)
        );
        assert_eq!(read.key, format!("a-{RUN}-1"));
        assert_eq!(read.status, Status::Done);
        assert_eq!(read.action, "search");
        assert_eq!(read.deed_ms, 10_000);
        assert_eq!(write.key, format!("a-{RUN}-2"));
        assert_eq!(write.status, Status::Prepared);
        assert_eq!(write.service, "gmail");
        assert_eq!(write.summary, "Your mail-count skill: gmail send_mail");
        let line = write.line();
        assert_eq!(line["source_kind"], "agent");
        assert_eq!(line["source_ref"]["service"], SOURCE_SERVICE);
        assert_eq!(line["source_ref"]["id"], format!("{RUN}:2"));
        assert_eq!(line["source_ref"]["session"], RUN);
        assert_eq!(line["deed_unix_ms"], 11_000);
        // A later call of the run is theirs too.
        pull(&[call_json(3, RUN, "service_read", "recent", "read", 14_000)]);
        assert_eq!(linked::waiting().len(), 3);
        assert_eq!(linked::waiting()[2].person, OWNER);
    }

    /// The read-back may come before the pull: the run is named when its
    /// calls arrive.
    #[test]
    fn the_read_back_may_come_first() {
        both_on();
        read_back(OWNER, "mail-count", "gmail", "s1", 12_000);
        assert!(wanted(12_000));
        assert!(named().is_empty());
        pull(&a_run(RUN, 1));
        assert_eq!(named(), [(RUN.to_owned(), OWNER.to_owned())]);
        assert_eq!(linked::waiting().len(), 2);
    }

    /// An unknown run is dropped: calls of a run no known person's
    /// read-back named make no row, are counted once let go, and a
    /// read-back to no known person, of another skill or code, or long
    /// after the held call, names nothing.
    #[test]
    fn an_unknown_run_is_dropped() {
        both_on();
        pull(&a_run(ELSE, 1));
        for person in ["", "the-owner", "room", "p-short"] {
            assert_eq!(
                read_back(person, "mail-count", "gmail", "s0", 12_000),
                ReadBackKept::NoPerson,
                "{person}"
            );
        }
        read_back(OWNER, "other-skill", "gmail", "s1", 12_000);
        read_back(OWNER, "mail-count", "google_calendar", "s2", 12_000);
        read_back(
            OWNER,
            "mail-count",
            "gmail",
            "s3",
            11_000 + HELD_BEFORE_MS + 1,
        );
        read_back(OWNER, "mail-count", "gmail", "s4", 10_500);
        assert!(named().is_empty());
        assert!(linked::waiting().is_empty());
        // Let go when older than a run lasts: counted, never written.
        assert!(wanted(11_000 + HELD_BEFORE_MS + 1));
        pulled(&json!({"calls": [], "last": 2}), 11_000 + RUN_FOR_MS + 1);
        assert_eq!(unknown(), 2);
        assert!(linked::waiting().is_empty());
        assert!(!wanted(11_000 + RUN_FOR_MS + 1));
    }

    /// Two runs: only the one whose write was read back is named; the
    /// other's calls stay unwritten.
    #[test]
    fn only_the_read_back_run_is_named() {
        both_on();
        let mut calls = a_run(ELSE, 1);
        calls.push(call_json(3, RUN, "service_read", "search", "read", 10_500));
        calls.push(call_json(
            4,
            RUN,
            "service_write",
            "send_mail",
            "held",
            11_500,
        ));
        pull(&calls);
        read_back(OWNER, "mail-count", "gmail", "s1", 12_000);
        assert_eq!(named(), [(RUN.to_owned(), OWNER.to_owned())]);
        assert_eq!(keys(), [format!("a-{RUN}-3"), format!("a-{RUN}-4")]);
    }

    /// A held write gives one row: `prepared` at its read-back, then the
    /// yes's outcome under the same key, written once; a later pull or
    /// settle writes nothing more.
    #[test]
    fn a_held_write_gives_one_row() {
        both_on();
        pull(&[call_json(
            1,
            RUN,
            "service_write",
            "send_mail",
            "held",
            11_000,
        )]);
        read_back(OWNER, "mail-count", "gmail", "s1", 12_000);
        assert_eq!(keys(), [format!("a-{RUN}-1")]);
        assert_eq!(linked::waiting()[0].status, Status::Prepared);
        // Pulled again before the yes: no second prepared row.
        let before = linked::waiting();
        pull(&[]);
        assert_eq!(linked::waiting(), before);
        sent("s1", &Got::Answered(&json!({"sent": true})), 15_000);
        let rows = linked::waiting();
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].key, format!("a-{RUN}-1"));
        assert_eq!(rows[0].status, Status::Done);
        assert_eq!(rows[0].changed_ms, 15_000);
        assert_eq!(rows[0].deed_ms, 11_000);
        // Nothing more: the read-back and the call are done with.
        settled("s1", Status::Failed, 16_000);
        pull(&[call_json(
            1,
            RUN,
            "service_write",
            "send_mail",
            "held",
            11_000,
        )]);
        assert_eq!(linked::waiting().len(), 1);
        assert_eq!(linked::waiting()[0].status, Status::Done);
    }

    /// The yes's outcomes: a no cancelled, not the owner's yes refused, a
    /// send not gone failed; settled before the pull, still one row.
    #[test]
    fn the_outcomes_of_the_yes() {
        for ((outcome, status), n) in [
            (Some(Status::Cancelled), Status::Cancelled),
            (Some(Status::Refused), Status::Refused),
            (None, Status::Failed),
        ]
        .into_iter()
        .zip(1u64..)
        {
            both_on();
            read_back(OWNER, "mail-count", "gmail", "s1", 12_000);
            match outcome {
                Some(outcome) => settled("s1", outcome, 13_000),
                None => sent("s1", &Got::Answered(&json!({"error": "no"})), 13_000),
            }
            pull(&[call_json(
                n,
                RUN,
                "service_write",
                "send_mail",
                "held",
                11_000,
            )]);
            let rows = linked::waiting();
            assert_eq!(rows.len(), 1, "{rows:?}");
            assert_eq!(rows[0].status, status);
            // The next case starts empty.
            forgot(None);
            linked::unsent(None);
        }
    }

    /// A refused call of a named run is `refused`; a skill's fixed write
    /// sent is `done`.
    #[test]
    fn refused_and_sent_calls() {
        both_on();
        let mut calls = a_run(RUN, 1);
        calls.push(call_json(
            3,
            RUN,
            "service_read",
            "search",
            "refused",
            11_200,
        ));
        calls.push(call_json(
            4,
            RUN,
            "service_write",
            "send_mail",
            "sent",
            11_300,
        ));
        pull(&calls);
        read_back(OWNER, "mail-count", "gmail", "s1", 12_000);
        let status: Vec<Status> = linked::waiting().iter().map(|row| row.status).collect();
        assert_eq!(
            status,
            [
                Status::Done,
                Status::Prepared,
                Status::Refused,
                Status::Done
            ]
        );
    }

    /// A call not as `lamp-services` gives it, or naming no catalog code,
    /// is passed over.
    #[test]
    fn a_call_not_as_given_is_passed_over() {
        let good = call_json(1, RUN, "service_read", "search", "read", 1);
        assert!(Call::of(&good).is_some());
        for (key, value) in [
            ("service", Value::Null),
            ("run", json!("a run")),
            ("tool", json!("connected")),
            ("n", json!("1")),
            ("action", json!("x".repeat(65))),
        ] {
            let mut bad = good.clone();
            bad[key] = value;
            assert!(Call::of(&bad).is_none(), "{key}");
        }
    }

    /// A forget of the person's everything lets their runs go, so a later
    /// call writes nothing; another person's stay.
    #[test]
    fn a_forget_lets_their_runs_go() {
        both_on();
        let mut calls = a_run(RUN, 1);
        calls.push(call_json(
            3,
            ELSE,
            "service_write",
            "send_mail",
            "held",
            11_500,
        ));
        pull(&calls);
        read_back(OWNER, "mail-count", "gmail", "s1", 11_200);
        read_back(OTHER, "mail-count", "gmail", "s2", 12_000);
        assert_eq!(named().len(), 2);
        crate::service_records::unsent(OWNER);
        assert_eq!(named(), [(ELSE.to_owned(), OTHER.to_owned())]);
        assert!(linked::waiting().iter().all(|row| row.person == OTHER));
        pull(&[call_json(5, RUN, "service_read", "search", "read", 14_000)]);
        assert!(linked::waiting().iter().all(|row| row.person == OTHER));
        crate::service_records::forgot(None);
        assert!(named().is_empty());
    }

    /// A forget about a thing lets only the runs it matches go.
    #[test]
    fn a_forget_about_lets_only_the_matching_runs_go() {
        both_on();
        let mut calls = a_run(RUN, 1);
        calls.push(call_json(
            3,
            RUN,
            "service_write",
            "send_mail",
            "held",
            11_500,
        ));
        pull(&calls);
        read_back(OWNER, "mail-count", "gmail", "s1", 12_000);
        crate::service_records::unsent_about(OWNER, &[vec!["calendar".into()]]);
        assert_eq!(named().len(), 1);
        crate::service_records::unsent_about(OWNER, &[vec!["send_mail".into()]]);
        assert!(named().is_empty());
        // The held write's row went with the forget, and its outcome is
        // never written now; the read, unmatched by it, stays.
        settled("s1", Status::Done, 13_000);
        assert_eq!(keys(), [format!("a-{RUN}-1")]);
    }

    /// At most 8 read-backs wait and 8 runs are named, the oldest let go
    /// first; at most 64 calls are kept.
    #[test]
    fn it_is_bounded() {
        both_on();
        for at in 0..READ_BACKS_MOST as u64 + 2 {
            read_back(OWNER, "mail-count", "gmail", &format!("s{at}"), 12_000);
        }
        assert_eq!(kept::kept(|kept| kept.read_backs.len()), READ_BACKS_MOST);
        let calls: Vec<Value> = (1..=CALLS_KEPT as u64 + 3)
            .map(|n| {
                call_json(
                    n,
                    &format!("task-1-1-{n}"),
                    "service_read",
                    "search",
                    "read",
                    10_000,
                )
            })
            .collect();
        pull(&calls);
        assert_eq!(kept::kept(|kept| kept.calls.len()), CALLS_KEPT);
        assert_eq!(unknown(), 3);
        assert_eq!(after(), CALLS_KEPT as u64 + 3);
        kept::kept(|kept| {
            for n in 0..RUNS_MOST + 2 {
                kept.runs.push_back(Named {
                    run: format!("r{n}"),
                    person: OWNER.into(),
                    at_ms: 12_000,
                });
                if kept.runs.len() > RUNS_MOST {
                    kept.runs.pop_front();
                }
            }
        });
        // The bound the naming keeps, by naming one more.
        pull(&[call_json(
            100,
            RUN,
            "service_write",
            "send_mail",
            "held",
            11_900,
        )]);
        read_back(OWNER, "mail-count", "gmail", "s-last", 12_000);
        let runs = named();
        assert!(runs.len() <= RUNS_MOST, "{}", runs.len());
        assert_eq!(runs.last().unwrap().0, RUN);
    }
}
