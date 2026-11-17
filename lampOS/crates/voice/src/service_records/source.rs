//! Find the source (`LAMP_SERVICE_RECORDS_SOURCE`, the linked-service
//! memory plan, slice 7): the `record_source` tool, which fetches what one
//! of the owner's service records points to, from the service that holds
//! it, when they ask for more than its one-line summary.
//!
//! - **Whose.** Only a line that passes the services' owner check
//!   (`services::owner_of`) is answered, and only for a key of that
//!   person's own: the memory reads the record by key under their id
//!   (`service_records` with `key`), so another person's key, or one a
//!   forget took, is [`NOT_YOURS`].
//! - **Who hears.** The told records' gate (`services::records_in_full`):
//!   in the private room, or the owner seen saying the line alone in view in
//!   their voice. Otherwise nothing is fetched and the model is told
//!   `told::BRIEF` ([`OTHERS_HERE`]).
//! - **Where.** A Harness task (`source_kind` `task`, `source_ref`
//!   `{service:"lamp-tasks", id:"<store>:<task>"}`) is read from lamp-tasks
//!   with `get_task`, then `get_observation` of its newest observation: a
//!   task the store no longer holds, or a store put back (another
//!   `store_id`), is [`NO_LONGER_AVAILABLE`]. An agent's row naming a
//!   `session` is never fetched: [`IN_AGENT`]. Every other kind (a
//!   connector record, a reminder, an agent's call without a session) has
//!   no read-by-id at its service that changes nothing (lamp-schedule's
//!   `get` marks a pending reminder offered), so what the record holds is
//!   told, said `from_record`.
//! - **Reads only.** Only `service_records`, `get_task` and
//!   `get_observation` are ever sent: no source is written to or deleted
//!   from. A source fetched marks its request as a service read does
//!   (`services::read_told`): nothing but another read runs after it, and
//!   its exchange is kept out of the thread and the running summary.
//! - **Bounded.** At most [`TOTAL`] a call, [`PER_CALL`] each service call
//!   (the memory's read too), and [`CALLS_A_TURN`] service calls in one
//!   request, the memory's read not counted: a call once they are spent
//!   asks nothing; the detail is at
//!   most [`DETAIL_BYTES`], marked as the service's data (`DATA_NOTE`),
//!   only its long field cut ([`CUT_FIRST`]) so the outcome is told whole.
//! - **Gone is gone.** [`GONE_RULE`]: a source no longer there is said so,
//!   never described from memory beyond its summary.
//!
//! Acts only with `LAMP_SERVICE_RECORDS` on too; off, the tool is not
//! offered and nobody runs it.
use serde_json::{Value, json};
use std::time::Duration;

/// Only the exact value `on`, with `LAMP_SERVICE_RECORDS` on, on the second
/// route.
pub const ENVIRONMENT: &str = "LAMP_SERVICE_RECORDS_SOURCE";
/// The tool's name.
pub const NAME: &str = "record_source";
/// The longest one `record_source` call takes, every service call in it.
pub const TOTAL: Duration = Duration::from_secs(4);
/// The longest one service call waits.
pub const PER_CALL: Duration = Duration::from_secs(3);
/// The most service calls (`get_task`, `get_observation`; the memory's
/// own read not counted) in one request.
pub const CALLS_A_TURN: u8 = 2;
/// The most bytes of the detail told.
pub const DETAIL_BYTES: usize = 1_500;
/// The most bytes of the memory's answer of one record.
const RECORD_ANSWER_BYTES: u64 = 32_768;

/// Why nothing was fetched.
pub const NO_LONGER_AVAILABLE: &str = "no_longer_available";
pub const TIMED_OUT: &str = "timed_out";
pub const NOT_YOURS: &str = "not_yours";
pub const IN_AGENT: &str = "in_agent";
pub const OTHERS_HERE: &str = "others_here";
pub const TOO_MANY: &str = "too_many_this_turn";

/// The rule told with a source that is gone.
pub const GONE_RULE: &str = "It is no longer available at that service: say so plainly. Never describe it from memory beyond its one-line summary, and never make up what it said or did.";
/// The rule told with a session kept by the agent.
pub const IN_AGENT_RULE: &str = "That work's conversation lives in the agent on the person's computer; the lamp keeps only a pointer to it. Say where it is, and nothing of what it said.";

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

/// Whether the tool is offered and run ([`ENVIRONMENT`]): only with
/// records kept.
pub fn on() -> bool {
    switch::on() && super::on()
}

/// Tests: the switch on or off on this thread.
#[cfg(test)]
pub fn on_for_test(on: bool) {
    switch::ON.with(|cell| cell.set(on));
}

pub fn is_tool(name: &str) -> bool {
    name == NAME
}

/// The tool, as the model is offered it.
pub fn tool() -> lamp_gemini::Tool {
    lamp_gemini::Tool {
        name: NAME.into(),
        description: format!(
            "Fetch more about one of the person's own service records (a task, an agent's work, a reminder or a service action) from the service that holds it, when they ask for more than its one-line summary (\"tell me more\", \"what did it say?\"). Give the record's key, as the records note or recall gives it. Only what it answers may be told. If it answers found false with reason {NO_LONGER_AVAILABLE}: {GONE_RULE}"
        ),
        parameters: Some(json!({
            "type": "object",
            "properties": {"key": {"type": "string", "description": "The record's key, exactly as given."}},
            "required": ["key"],
        })),
        silent: false,
    }
}

/// Where the call reads, each a bounded exchange given its request and how
/// long it may wait; `None`, no answer in that time.
pub trait Sources {
    fn memory(&mut self, line: &Value, wait: Duration) -> Option<Value>;
    fn tasks(&mut self, line: &Value, wait: Duration) -> Option<Value>;
}

/// The lamp's sockets: the Days memory, and lamp-tasks' request socket
/// when the tasks part is set.
pub struct Sockets<'a> {
    pub memory: &'a std::path::Path,
    pub tasks: Option<&'a std::path::Path>,
}

impl Sources for Sockets<'_> {
    fn memory(&mut self, line: &Value, wait: Duration) -> Option<Value> {
        crate::tools::exchange_up_to(self.memory, line, wait, RECORD_ANSWER_BYTES)
    }

    fn tasks(&mut self, line: &Value, wait: Duration) -> Option<Value> {
        let socket = self.tasks?.join("request.sock");
        crate::tools::exchange_up_to(&socket, line, wait, crate::tools::MAX_TASKS_ANSWER_BYTES)
    }
}

/// Who asks: the person's memory id (`services::owner_of`, then
/// `told::person_for`), none for anyone else; and whether they may hear it
/// in full.
#[derive(Clone, Debug)]
pub struct Asker {
    pub person: Option<String>,
    pub in_full: bool,
}

/// The service calls this request has made ([`CALLS_A_TURN`]).
#[derive(Default)]
pub struct Turn {
    request: u64,
    calls: u8,
}

impl Turn {
    /// Whether `request` has made its [`CALLS_A_TURN`] already.
    fn spent(&self, request: u64) -> bool {
        self.request == request && self.calls >= CALLS_A_TURN
    }

    /// Room for one more call in `request`: counted when there is.
    fn take(&mut self, request: u64) -> bool {
        if self.request != request {
            self.request = request;
            self.calls = 0;
        }
        if self.calls >= CALLS_A_TURN {
            return false;
        }
        self.calls += 1;
        true
    }
}

#[cfg(not(test))]
fn with_turn<R>(f: impl FnOnce(&mut Turn) -> R) -> R {
    use std::sync::{LazyLock, Mutex};
    static TURN: LazyLock<Mutex<Turn>> = LazyLock::new(Mutex::default);
    f(&mut TURN.lock().unwrap_or_else(|poisoned| poisoned.into_inner()))
}

#[cfg(test)]
fn with_turn<R>(f: impl FnOnce(&mut Turn) -> R) -> R {
    use std::cell::RefCell;
    thread_local! {
        static TURN: RefCell<Turn> = RefCell::default();
    }
    TURN.with(|turn| f(&mut turn.borrow_mut()))
}

fn not_found(reason: &str) -> Value {
    let mut told = json!({"found": false, "reason": reason});
    match reason {
        NO_LONGER_AVAILABLE => told["rule"] = json!(GONE_RULE),
        IN_AGENT => told["rule"] = json!(IN_AGENT_RULE),
        OTHERS_HERE => told["note"] = json!(super::told::BRIEF),
        _ => {}
    }
    told
}

/// The long fields, cut in this order when the detail is over
/// [`DETAIL_BYTES`]: the agent's answer, a record's detail, then what was
/// asked. The short ones (`outcome`, `state`, `status`, `service`,
/// `action`) are always told whole.
const CUT_FIRST: [&str; 3] = ["answer", "detail", "asked"];

/// The detail told: `value` as JSON text of at most [`DETAIL_BYTES`],
/// marked as the service's data. Over that, only its long fields are cut
/// ([`CUT_FIRST`]), each at a character to the most that fits, and named in
/// `cut`, so the text stays whole JSON.
fn detail_of(value: &Value) -> Value {
    let mut value = value.clone();
    let mut cut: Vec<&str> = Vec::new();
    for field in CUT_FIRST {
        if value.to_string().len() <= DETAIL_BYTES {
            break;
        }
        let Some(long) = value.get(field).filter(|long| !long.is_null()) else {
            continue;
        };
        let chars: Vec<char> = match long {
            Value::String(text) => text.chars().collect(),
            other => other.to_string().chars().collect(),
        };
        cut.push(field);
        value["cut"] = json!(cut);
        let fits = |value: &mut Value, n: usize| {
            value[field] = Value::String(chars[..n].iter().collect());
            value.to_string().len() <= DETAIL_BYTES
        };
        // The most characters of it that fit.
        let (mut low, mut high) = (0, chars.len());
        while low < high {
            let mid = (low + high).div_ceil(2);
            if fits(&mut value, mid) {
                low = mid;
            } else {
                high = mid - 1;
            }
        }
        fits(&mut value, low);
    }
    let mut text = value.to_string();
    // Short fields alone over the bound (never from lamp-tasks or a
    // record as kept): the text itself is cut at a character.
    if text.len() > DETAIL_BYTES {
        let mut end = DETAIL_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    json!({"text": text, "note": crate::services::DATA_NOTE})
}

/// A source fetched in `request` is a service's data: marked as a
/// service read is (`services::read_told`), so nothing but another read
/// runs after it and its exchange is kept out of the thread and the
/// running summary. Nothing fetched marks nothing.
fn told_as_read(request: u64, told: &Value) {
    if told["found"] == true {
        crate::services::read_told(request);
    }
}

/// Answer one `record_source` call of `request` for `asker`, reading
/// through `sources`, `elapsed` the time since the call began. Reads only.
pub fn answer(
    sources: &mut impl Sources,
    asker: &Asker,
    request: u64,
    args: &Value,
    elapsed: &dyn Fn() -> Duration,
) -> Value {
    let Some(person) = asker.person.as_deref() else {
        return not_found(NOT_YOURS);
    };
    let Some(key) = args
        .get("key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|key| !key.is_empty() && key.len() <= 128)
    else {
        return not_found(NOT_YOURS);
    };
    if !asker.in_full {
        return not_found(OTHERS_HERE);
    }
    // What is left of the call's time, at most one service call's.
    let wait = || {
        TOTAL
            .checked_sub(elapsed())
            .map(|left| left.min(PER_CALL))
            .filter(|left| !left.is_zero())
    };
    // This request's service calls spent: nothing is asked, the memory
    // neither.
    if with_turn(|turn| turn.spent(request)) {
        return not_found(TOO_MANY);
    }
    // A service call, counted in the turn's [`CALLS_A_TURN`].
    let call = |sources: &mut dyn FnMut(Duration) -> Option<Value>| {
        if !with_turn(|turn| turn.take(request)) {
            return Err(TOO_MANY);
        }
        let wait = wait().ok_or(TIMED_OUT)?;
        sources(wait).ok_or(TIMED_OUT)
    };
    // The memory's own read is not a service call.
    let memory = |sources: &mut dyn FnMut(Duration) -> Option<Value>| {
        let wait = wait().ok_or(TIMED_OUT)?;
        sources(wait).ok_or(TIMED_OUT)
    };
    let line = json!({"op":"service_records","person":person,"key":key});
    let record = match memory(&mut |wait| sources.memory(&line, wait)) {
        Ok(answer) if answer["ok"] == true => answer["records"]
            .as_array()
            .and_then(|records| records.first())
            .cloned(),
        // An older memory refuses `key`: nothing could be read now.
        Ok(_) => return not_found(TIMED_OUT),
        Err(reason) => return not_found(reason),
    };
    let Some(record) = record.filter(|record| record["key"] == key) else {
        return not_found(NOT_YOURS);
    };
    let kind = record["source_kind"].as_str().unwrap_or("voice").to_owned();
    let summary = record["summary"].as_str().unwrap_or_default().to_owned();
    let source = &record["source_ref"];
    let found = |detail: Value, from: &str| json!({"found": true, "kind": kind, "summary": summary, "from": from, "detail": detail});
    match kind.as_str() {
        "task" => {
            let Some((store, id)) = source["id"]
                .as_str()
                .and_then(|id| id.rsplit_once(':'))
                .and_then(|(store, id)| Some((store.to_owned(), id.parse::<u64>().ok()?)))
            else {
                return not_found(NO_LONGER_AVAILABLE);
            };
            let ask = json!({"op":"get_task","id":id});
            let task = match call(&mut |wait| sources.tasks(&ask, wait)) {
                Ok(task) => task,
                Err(reason) => return not_found(reason),
            };
            if task["ok"] != true || task["store_id"] != store.as_str() {
                return not_found(NO_LONGER_AVAILABLE);
            }
            let newest = task["observations"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|observation| observation["id"].as_u64())
                .max();
            let mut detail = json!({
                "asked": task["text"],
                "state": task["task"]["dispatch"]["state"],
            });
            if let Some(observation_id) = newest {
                let ask = json!({"op":"get_observation","id":id,"observation_id":observation_id});
                match call(&mut |wait| sources.tasks(&ask, wait)) {
                    Ok(observed) if observed["ok"] == true => {
                        detail["outcome"] = observed["observation"]["state"].clone();
                        detail["answer"] = observed["data"]["answer"].clone();
                    }
                    // The task is there; its answer could not be read now.
                    Ok(_) | Err(_) => detail["answer_unread"] = json!(true),
                }
            }
            found(detail_of(&detail), "lamp-tasks")
        }
        "agent" if source.get("session").is_some() => not_found(IN_AGENT),
        _ => {
            let mut held = json!({
                "service": record["service"],
                "action": record["action"],
                "status": record["status"],
            });
            if let Some(detail) = record.get("detail").filter(|detail| !detail.is_null()) {
                held["detail"] = detail.clone();
            }
            found(detail_of(&held), "record")
        }
    }
}

/// [`answer`] on the lamp's sockets, for the line `request` (its memory
/// scope `lease`): the owner's check and the told records' gate.
pub fn run(
    memory: &std::path::Path,
    tasks: Option<&std::path::Path>,
    args: &Value,
    request: u64,
    lease: Option<&crate::memory_scope::ScopeLease>,
) -> (Value, bool) {
    if !on() {
        return (crate::tools::unavailable(), false);
    }
    let owner = crate::services::owner_of(lease);
    let person = owner.as_deref().and_then(super::told::person_for);
    let in_full = match (&owner, lease) {
        (Some(owner), Some(lease)) => crate::services::records_in_full(lease.key().request, owner),
        (Some(_), None) => crate::person::private_room(),
        _ => false,
    };
    let began = std::time::Instant::now();
    let told = answer(
        &mut Sockets { memory, tasks },
        &Asker { person, in_full },
        request,
        args,
        &|| began.elapsed(),
    );
    told_as_read(request, &told);
    eprintln!(
        "lamp-voice: request {request}: record_source found {}{} in {} ms (LAMP_SERVICE_RECORDS_SOURCE)",
        told["found"],
        told["reason"]
            .as_str()
            .map(|reason| format!(", {reason}"))
            .unwrap_or_default(),
        began.elapsed().as_millis()
    );
    (told, true)
}

#[cfg(test)]
mod tests;
