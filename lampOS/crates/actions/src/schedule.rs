//! Opt-in model tools for Schedule. One validated call gets one bounded attempt;
//! a lost mutation reply is unknown, never grounds for a new-guard retry.
use crate::Function;
use lamp_schedule::{
    client,
    protocol::{Asker, Guard, Request},
    time::{
        Deadline, Fold, MAX_AFTER_SECONDS, MAX_EVERY_MINUTES, MIN_EVERY_MINUTES, Repeat, Weekday,
        When, validate_zone,
    },
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    ffi::OsStr,
    io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub const DEFAULT_SOCKET: &str = "/run/lampos-schedule/call.sock";
/// `on`: `repeat_reminder` also takes a reminder every so many minutes or
/// hours (`interval`), as `lamp-schedule` with the same switch keeps one.
pub const EVERY_SWITCH: &str = "LAMP_SCHEDULE_EVERY";

/// Whether [`EVERY_SWITCH`] is `on` in this process's environment.
pub fn every_from_environment() -> bool {
    std::env::var_os(EVERY_SWITCH).as_deref() == Some(OsStr::new("on"))
}
/// `on`: a list asked by the room's line (no person) also tells how many
/// reminders and timers it left out as someone's own, a count only, with
/// [`HIDDEN`] (the memory owner's option 2, 4 Nov, 29e7's "only call your
/// dad"). Off, the list is exactly what it was.
pub const HIDDEN_SWITCH: &str = "LAMP_REMINDERS_HIDDEN_COUNT";

/// Whether [`HIDDEN_SWITCH`] is `on` in this process's environment.
pub fn hidden_from_environment() -> bool {
    std::env::var_os(HIDDEN_SWITCH).as_deref() == Some(OsStr::new("on"))
}
pub const DEADLINE: Duration = Duration::from_millis(1250);
/// Told beside a list that left out `hidden` reminders or timers as
/// someone's own ([`HIDDEN_SWITCH`]): never told as the whole list, and
/// nothing of them guessed.
pub const HIDDEN: &str = "hidden: how many more reminders or timers are set that belong to someone, left out of this list. Never call this list all that is set, or say none are set, and never guess what they are, when, or whose. Say there are that many more that belong to someone, and if they are theirs, to ask again where you can see their face.";
/// Told beside a list asked `pending_only` that left out items set but not
/// yet due (#1595).
pub const NOT_YET_DUE: &str = "Timers or reminders are set that are not due yet, so they are not in this list. List again without pending_only before saying none are set, or to find one to cancel.";

macro_rules! list_before {
    () => {
        "Read one bounded page of active timers and reminders, oldest first. Each item's kind is timer (set with set_timer) or reminder. To say what is set, or to find one to cancel, leave pending_only out. If next_after_id is present, use it as after_id to read more; never describe an incomplete page as the whole list. If not_yet_due, how many are set and not yet due in all, is more than 0, timers or reminders are set: list again without pending_only before saying none are."
    };
}
macro_rules! list_asked {
    () => {
        "Call this, not recall, for any question about their reminders or timers: what is set, when one is due, or when they set it; never say you do not remember a reminder before calling this. set_day and set_clock say when each was set, as 'yesterday at 09:00'; if they are null, say you don't know when. Never list reminders unasked."
    };
}
macro_rules! list_said {
    () => {
        "said_recently: ones already said; said_day/said_clock: when you said it; waiting_for_answer: said late, unanswered. Asked what was set earlier, use both lists: 'I reminded you at 10:49 about the report', never none set."
    };
}
/// `list_reminders`' description before questions about reminders were
/// named in it: kept for the probe's old arm (`lamp-voice probe-tools`).
pub const LIST_BEFORE: &str = list_before!();
/// What `list_reminders`' description adds (the person's call, 30 Oct): a
/// question about a reminder, what is set, when it is due or when it was
/// set, is answered from this list, not from memory, and the list is never
/// read out unasked (rule 8). On both routes: they declare the same catalog.
pub const LIST_ASKED: &str = list_asked!();
/// What it adds for reminders already said (29e7, 2 Nov: four said late,
/// then gone, and six minutes later "I don't see either still set").
pub const LIST_SAID: &str = list_said!();
/// `list_reminders`' description.
pub const LIST_DESCRIBED: &str = concat!(list_before!(), " ", list_asked!(), " ", list_said!());

/// Voice reserves 16 bytes in its provider's 4096-byte tool-result allowance.
pub const MAX_RESULT_BYTES: usize = 4096 - 16;
pub const NAMES: [&str; 6] = [
    "set_timer",
    "set_reminder",
    "repeat_reminder",
    "list_reminders",
    "cancel_reminder",
    "snooze_reminder",
];

pub fn is_tool(name: &str) -> bool {
    NAMES.contains(&name)
}

/// The existing provider's small Schema vocabulary. Runtime validation below
/// is stricter than a model schema; byte limits and unknown fields are checked.
pub fn catalog() -> Vec<Function> {
    catalog_with(every_from_environment())
}

/// [`catalog`] with [`EVERY_SWITCH`] on (`every`) or off: off, it is the
/// catalog as it was, byte for byte.
pub fn catalog_with(every: bool) -> Vec<Function> {
    let text = json!({"type":"string","description":"A short label: 1 to 300 UTF-8 bytes, without control characters."});
    let seconds = json!({"type":"integer","minimum":1,"maximum":MAX_AFTER_SECONDS});
    let time =
        json!({"type":"string","description":"Local time in exact HH:MM, 00:00 through 23:59."});
    let zone = json!({"type":"string","description":"An IANA time-zone name, only if specified; otherwise use the lamp's configured zone."});
    let id = json!({"type":"integer","minimum":1,"description":"An exact ID from list_reminders; never guess one."});
    let function = |name, description, properties, required| Function {
        name,
        description,
        parameters: Some(json!({"type":"object","properties":properties,"required":required})),
    };
    vec![
        function(
            NAMES[0],
            "Set a timer or a relative reminder only when asked. For a duration from now, such as 'in two minutes', 'in ten minutes', or 'in an hour', use this tool with the requested label and whole seconds (120, 600, or 3600 respectively); never round to a clock time. Confirm the returned time only after durable success. If the outcome is unknown, list current reminders before asking again. If they change the time of one already set, cancel that one before setting the new one; never leave both.",
            json!({"text":text,"seconds":seconds}),
            json!(["text", "seconds"]),
        ),
        function(
            NAMES[1],
            "Set one reminder at a requested local clock time, such as 'at nine tomorrow morning'. For relative times from now, use set_timer with the requested label and whole seconds instead; do not round to HH:MM. Ask for missing or ambiguous intent; never calculate a Unix timestamp.",
            json!({"text":text,"time":time,"day":{"type":"string","description":"today, tomorrow, next, or exact YYYY-MM-DD; omitted means the next strictly future occurrence."},"zone":zone,"fold":{"type":"string","enum":["earlier","later","reject"]}}),
            json!(["text", "time"]),
        ),
        if every {
            function(
                NAMES[2],
                "Repeat a reminder daily or weekly at the requested local time, or every so many minutes or hours. Weekly needs a weekday; daily must omit it; both need time. For 'every hour', 'every 2 hours' or 'every 20 minutes', use every interval with whole minutes or whole hours, never both, from 15 minutes to 24 hours, and no time; asked for less than every 15 minutes, set nothing yet: say plainly that every 15 minutes is the shortest, and ask if that will do; with a span such as 'from 8 to 6', add from and to as HH:MM (either order; one across midnight is fine). 'Stop reminding me' cancels the whole series with cancel_reminder.",
                json!({"text":text,"time":time,"every":{"type":"string","enum":["daily","weekly","interval"]},"weekday":{"type":"string","enum":["monday","tuesday","wednesday","thursday","friday","saturday","sunday"]},"minutes":{"type":"integer","minimum":MIN_EVERY_MINUTES,"maximum":MAX_EVERY_MINUTES},"hours":{"type":"integer","minimum":1,"maximum":MAX_EVERY_MINUTES / 60},"from":time,"to":time,"zone":zone}),
                json!(["text", "every"]),
            )
        } else {
            function(
                NAMES[2],
                "Repeat a reminder daily or weekly at the requested local time. Weekly needs a weekday; daily must omit it.",
                json!({"text":text,"time":time,"every":{"type":"string","enum":["daily","weekly"]},"weekday":{"type":"string","enum":["monday","tuesday","wednesday","thursday","friday","saturday","sunday"]},"zone":zone}),
                json!(["text", "time", "every"]),
            )
        },
        function(
            NAMES[3],
            LIST_DESCRIBED,
            json!({"pending_only":{"type":"boolean","description":"true lists only items that are due now and not yet said; leave it out to list everything that is set."},"after_id":{"type":"integer","minimum":0}}),
            json!([]),
        ),
        function(
            NAMES[4],
            "Cancel a timer or a whole reminder series the person set, only when asked: \"stop reminding me to drink water\", \"cancel my water reminder\", \"no more pasta timer\". Give its id from the current list (read without pending_only), or, with no id, its text: the words of what it reminds them of (\"drink water\"). By text, the one item whose text contains them is cancelled; if several do, or none, you are told which, and nothing is cancelled. For a timer, cancel only an item whose kind is timer; for a reminder, only one whose kind is reminder. A reminder the person set is never one of the lamp's own wellbeing nudges: never answer this with wellbeing_nudges.",
            json!({"id":id,"text":{"type":"string","description":"What the reminder or timer is for, in the person's words (\"drink water\"), when no id is known."}}),
            json!([]),
        ),
        function(
            NAMES[5],
            "Postpone the selected currently due occurrence by whole seconds. This preserves a repeating reminder's original schedule. Use an ID from the current list.",
            json!({"id":id,"seconds":seconds}),
            json!(["id", "seconds"]),
        ),
    ]
}

/// Who a voice line's schedule call is for (Reviewer-three's after-merge P2
/// on #4527, the first builder's (a)): whose reminders it hears, cancels and
/// snoozes ([`Asker`]), and who a reminder it sets is kept as. Sent by the
/// voice beside each call on the executive's socket; a call without one is
/// nobody's: it hears and changes only the room's (set by no one, the first
/// builder on #4710, 3 Nov 10:09), and what it sets is the room's. The app's
/// own calls ([`Adapter::call`], [`Adapter::call_plain`]) take none and see
/// every item, as the owner's.
#[derive(Clone, Debug, Default, Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Caller {
    #[serde(default)]
    pub asker: Asker,
    /// The memory person id a keep in this line would be written for; none
    /// when nobody is sure.
    #[serde(default)]
    pub setter: Option<String>,
    /// The line sees and changes every item, as the app's own calls do, and
    /// what it sets is everyone's: the voice's private room
    /// (`LAMP_PRIVATE_ROOM`, the owner's call on 6 Nov).
    #[serde(default)]
    pub everyone: bool,
}

#[derive(Clone, Debug)]
pub struct Adapter {
    socket: PathBuf,
    /// [`DEADLINE`] for every product path.
    deadline: Duration,
    /// [`EVERY_SWITCH`]: `repeat_reminder` takes `interval`.
    every: bool,
    /// [`HIDDEN_SWITCH`]: the room's list tells how many it left out.
    hidden: bool,
}

impl Adapter {
    pub fn new(socket: PathBuf) -> io::Result<Self> {
        client::validate_socket(&socket).map_err(io::Error::other)?;
        Ok(Self {
            socket,
            deadline: DEADLINE,
            every: every_from_environment(),
            hidden: hidden_from_environment(),
        })
    }

    /// The same adapter with [`EVERY_SWITCH`] taken as `every`, for a test.
    pub fn with_every(mut self, every: bool) -> Self {
        self.every = every;
        self
    }

    /// The same adapter with [`HIDDEN_SWITCH`] taken as `hidden`, for a test.
    pub fn with_hidden(mut self, hidden: bool) -> Self {
        self.hidden = hidden;
        self
    }

    /// The same adapter, giving one call `deadline` instead of [`DEADLINE`]:
    /// for a host test against a real Schedule service on a loaded machine,
    /// where one durable commit can take past 1250 ms (lamp-voice's push
    /// runs on 29 Oct). Never used on a lamp.
    pub fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }

    /// An absent or inexact switch does not even inspect the socket setting.
    pub fn configured(switch: Option<&OsStr>, socket: Option<&Path>) -> io::Result<Option<Self>> {
        if switch != Some(OsStr::new("on")) {
            return Ok(None);
        }
        Self::new(
            socket
                .unwrap_or_else(|| Path::new(DEFAULT_SOCKET))
                .to_owned(),
        )
        .map(Some)
    }

    pub fn from_environment() -> io::Result<Option<Self>> {
        let switch = std::env::var_os("LAMP_SCHEDULE_TOOLS");
        if switch.as_deref() != Some(OsStr::new("on")) {
            return Ok(None);
        }
        let socket = std::env::var_os("LAMP_SCHEDULE_SOCKET");
        Self::configured(switch.as_deref(), socket.as_deref().map(Path::new))
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Returns the ordinary Actions envelope. Successful result objects fit
    /// Voice's separate model-result budget. Failure sentences are fixed and
    /// never contain the caller's reminder text or an owner's arbitrary error.
    pub fn call(&self, name: &str, args: &Value) -> Value {
        self.call_with(name, args, true, None)
    }

    /// [`Self::call`] for a voice line: only `caller`'s reminders are
    /// listed, cancelled or snoozed, and one it sets is kept as its setter's.
    pub fn call_for(&self, name: &str, args: &Value, caller: &Caller) -> Value {
        self.call_with(name, args, true, (!caller.everyone).then_some(caller))
    }

    /// Every reminder and timer `person` (a memory person id) set,
    /// cancelled: their "forget everything about me", or the app removing
    /// them entirely. `{"ok":true,"result":{"forgotten":n}}`, or a fixed
    /// failure, as [`Self::call`] gives one.
    pub fn forget_person(&self, person: &str) -> Value {
        let deadline = Instant::now() + self.deadline;
        let forgot = (|| {
            if !lamp_msgs::known::is_person_id(person) {
                return Err(Failure::BadArguments);
            }
            let before = self.ask(&Request::Status {}, deadline)?;
            let (store_id, revision) = identity(&before)?;
            let request = Request::Forget {
                guard: guard(store_id, revision)?,
                person: person.to_owned(),
            };
            let reply = self.ask(&request, deadline)?;
            let (store, after) = identity(&reply).map_err(|_| Failure::Uncertain)?;
            let forgotten = reply["forgotten"].as_u64();
            match (request.guard(), forgotten) {
                (Some(guard), Some(forgotten))
                    if store == guard.store_id && guard.revision.checked_add(1) == Some(after) =>
                {
                    Ok(json!({"forgotten":forgotten}))
                }
                _ => Err(Failure::Uncertain),
            }
        })();
        match forgot {
            Ok(result) => json!({"ok":true,"result":result}),
            Err(why) => failure(why),
        }
    }

    /// [`Self::call`] with `list_reminders` asked as a plain `list`, its
    /// items without when they were set: the app's `schedule.list`, whose
    /// contract is unchanged. Only Voice's tool asks `list_set`.
    pub fn call_plain(&self, name: &str, args: &Value) -> Value {
        self.call_with(name, args, false, None)
    }

    fn call_with(
        &self,
        name: &str,
        args: &Value,
        set_times: bool,
        caller: Option<&Caller>,
    ) -> Value {
        let deadline = Instant::now() + self.deadline;
        match parse(name, args, self.every)
            .and_then(|call| self.perform(call, set_times, caller, deadline))
        {
            Ok(result) if result.to_string().len() <= MAX_RESULT_BYTES => {
                json!({"ok":true,"result":result})
            }
            Ok(_) => failure(Failure::Uncertain),
            Err(why) => failure(why),
        }
    }

    fn ask(&self, request: &Request, deadline: Instant) -> Result<Value, Failure> {
        let mutation = request.guard().is_some();
        self.ask_raw(request, deadline)?
            .map_err(|refusal| refused(&refusal, mutation))
    }

    /// [`Self::ask`] with a refusal given back as it came.
    fn ask_raw(
        &self,
        request: &Request,
        deadline: Instant,
    ) -> Result<Result<Value, Value>, Failure> {
        let mutation = request.guard().is_some();
        let value = serde_json::to_value(request).map_err(|_| Failure::Unavailable)?;
        let reply = client::exchange(&self.socket, &value, deadline).map_err(|e| {
            if mutation && e.may_have_sent {
                Failure::Uncertain
            } else {
                Failure::Unavailable
            }
        })?;
        match reply["ok"].as_bool() {
            Some(true) => {
                identity(&reply).map_err(|_| invalid_reply(mutation))?;
                Ok(Ok(reply))
            }
            Some(false) => Ok(Err(reply)),
            None => Err(invalid_reply(mutation)),
        }
    }

    fn perform(
        &self,
        call: Call,
        set_times: bool,
        caller: Option<&Caller>,
        deadline: Instant,
    ) -> Result<Value, Failure> {
        let asker = caller.map(|caller| caller.asker.clone());
        if let Call::List {
            after_id,
            pending_only,
        } = call
        {
            // With each item's set time; an older Schedule refuses
            // `list_set` as a bad request, and the same list without it is
            // asked once (a read: nothing to repeat), told as set at an
            // unknown time.
            let plain = Request::List {
                after_id,
                limit: 8,
                pending_only,
            };
            let reply = if !set_times {
                self.ask(&plain, deadline)?
            } else {
                match self.ask_raw(
                    &Request::ListSet {
                        after_id,
                        limit: 8,
                        pending_only,
                        asker: asker.clone(),
                    },
                    deadline,
                )? {
                    Ok(reply) => reply,
                    // An older Schedule keeps no setters and takes no
                    // asker: only the owner's line lists its items, as
                    // before (#4527's P2).
                    Err(refusal)
                        if refusal["error"] == "bad_request"
                            && asker.as_ref().is_some_and(|asker| !asker.owner) =>
                    {
                        let mut none = self.ask(&Request::Status {}, deadline)?;
                        none["items"] = json!([]);
                        none["next_after_id"] = Value::Null;
                        none
                    }
                    Err(refusal) if refusal["error"] == "bad_request" => {
                        self.ask(&plain, deadline)?
                    }
                    Err(refusal) => return Err(refused(&refusal, false)),
                }
            };
            // How many the room's line left out as someone's own, with
            // the switch on: a Schedule without the count tells none.
            let hidden = reply["hidden"]
                .as_u64()
                .filter(|_| self.hidden && asker.is_some());
            let mut listed = page(&reply, after_id, pending_only, set_times, hidden.is_some())?;
            if let Some(hidden) = hidden.filter(|hidden| *hidden > 0) {
                listed["hidden"] = json!(hidden);
                listed["hidden_note"] = json!(HIDDEN);
            }
            // A line's own count of what is set and not yet due, from the
            // same reply, never the whole store's (#4527's P2).
            // An older Schedule's reply has none: the owner's line, the only
            // one it lists for, is counted from `Status` as before.
            let own_waiting = reply["not_yet_due"].as_u64().filter(|_| asker.is_some());
            let counts_all = asker.as_ref().is_none_or(|asker| asker.owner);
            if let Some(waiting) = own_waiting {
                if pending_only && waiting > 0 {
                    listed["not_yet_due"] = json!(waiting);
                    listed["next"] = json!(NOT_YET_DUE);
                }
            } else if pending_only && counts_all {
                // #1595: a list of what is due leaves out what is set and not
                // yet due, and "nothing due" read as "nothing set" let a timer
                // the person asked to cancel ring. Said as a count, best
                // effort: the list itself stands if the count can't be read.
                if let Some(waiting) = self
                    .ask(&Request::Status {}, deadline)
                    .ok()
                    .and_then(|status| {
                        Some(
                            status["items"]
                                .as_u64()?
                                .saturating_sub(status["pending"].as_u64()?),
                        )
                    })
                    .filter(|waiting| *waiting > 0)
                {
                    listed["not_yet_due"] = json!(waiting);
                    listed["next"] = json!(NOT_YET_DUE);
                }
            }
            return Ok(listed);
        }
        // By text: the asker's own list, read as list_reminders reads it,
        // then the one match cancelled by id, as any cancel is.
        let call = match call {
            Call::CancelText { text } => {
                let mut matches = Vec::new();
                let mut after_id = 0;
                for _ in 0..MOST_TEXT_PAGES {
                    let listed = self.perform(
                        Call::List {
                            after_id,
                            pending_only: false,
                        },
                        set_times,
                        caller,
                        deadline,
                    )?;
                    let items = listed["items"].as_array().cloned().unwrap_or_default();
                    matches.extend(items.iter().filter(|item| by_text(item, &text)).cloned());
                    match (
                        listed["complete"].as_bool(),
                        listed["next_after_id"].as_u64(),
                    ) {
                        (Some(false), Some(next)) if next > after_id => after_id = next,
                        _ => break,
                    }
                }
                match matches.as_slice() {
                    [one] => Call::Cancel {
                        id: one["id"].as_u64().ok_or(Failure::Unavailable)?,
                    },
                    _ => {
                        let found: Vec<Value> = matches
                            .iter()
                            .map(|item| json!({"id":item["id"],"kind":item["kind"],"text":item["text"],"repeat":item["repeat"]}))
                            .collect();
                        let next = if found.is_empty() {
                            "Nothing they set matches those words: say so in a few words, and cancel nothing."
                        } else {
                            "More than one matches: ask which, naming each in a few words, and cancel by its id once they say."
                        };
                        return Ok(json!({"cancelled": false, "matches": found, "next": next}));
                    }
                }
            }
            call => call,
        };
        // Snooze obtains guard and delivery from the SAME Get reply. A
        // line's set reads its own first page, which says what it has set
        // already with the same words (29e7, 2 Nov: a second water timer set
        // beside the first). Other mutations use Status. No conflict or
        // uncertain mutation is retried.
        let before = match (&call, &asker) {
            (Call::Snooze { id, .. }, _) => self.ask(&Request::Get { id: *id }, deadline)?,
            (Call::Set { .. }, Some(asker)) => match self.ask_raw(
                &Request::ListSet {
                    after_id: 0,
                    limit: 8,
                    pending_only: false,
                    asker: Some(asker.clone()),
                },
                deadline,
            )? {
                Ok(reply) => reply,
                // An older Schedule: as before.
                Err(_) => self.ask(&Request::Status {}, deadline)?,
            },
            _ => self.ask(&Request::Status {}, deadline)?,
        };
        // The same timer asked for again in a moment (29e7's ids 8 and 9):
        // the one already set, and nothing new.
        if let (
            Call::Set {
                text,
                when: When::After { seconds },
            },
            Some(_),
        ) = (&call, &asker)
            && let Some(set) = same_timer(&before, text, *seconds)
        {
            return Ok(set);
        }
        let same = match &call {
            Call::Set { text, .. } => same_words_set(&before, text),
            _ => Vec::new(),
        };
        let (store_id, revision) = identity(&before)?;
        let guard = guard(store_id, revision)?;
        let request = match call {
            Call::Set { text, when } => Request::Set {
                guard,
                text,
                when,
                person: caller.and_then(|caller| caller.setter.clone()),
            },
            Call::Cancel { id } => Request::Cancel { guard, id, asker },
            Call::Snooze { id, seconds } => {
                let item = item(&before["item"]).map_err(|_| Failure::Unavailable)?;
                if item.id != id {
                    return Err(Failure::Unavailable);
                }
                // For a line, Schedule says first whether it is theirs at
                // all: someone else's is not found, never "not due".
                if item.state != "pending" && asker.is_none() {
                    return Err(Failure::NotPending);
                }
                Request::Snooze {
                    guard,
                    id,
                    delivery: item.delivery,
                    seconds,
                    asker,
                }
            }
            Call::List { .. } | Call::CancelText { .. } => {
                unreachable!("list and a cancel by text resolved before mutation")
            }
        };
        let response = self.ask(&request, deadline)?;
        let mut done = committed(&request, &response)?;
        let new = done["id"].as_u64();
        let same: Vec<_> = same
            .into_iter()
            .filter(|one| one["id"].as_u64() != new)
            .collect();
        if done["status"] == "committed" && !same.is_empty() {
            done["same_words_set"] = json!(same);
            done["note"] = json!(SAME_WORDS);
        }
        Ok(done)
    }
}

/// A line asking for the same timer again within this long of setting it,
/// for the same length, gets the one already set (29e7, 2 Nov: two water
/// timers 29 s apart).
pub const SAME_SET_WITHIN_MS: i64 = 2 * 60_000;

/// The timer in a `list_set` page (`page`) that is `text` for `seconds`,
/// set within [`SAME_SET_WITHIN_MS`] before the page was read
/// (`now_unix_ms`, Schedule's clock) and not yet due, snoozed or said, as a
/// set's result: `already_set`. None in a reply without set times (an
/// older Schedule).
fn same_timer(page: &Value, text: &str, seconds: u64) -> Option<Value> {
    let now_ms = page["now_unix_ms"].as_i64()?;
    let length = i64::try_from(seconds.checked_mul(1_000)?).ok()?;
    let item = page["items"].as_array()?.iter().find(|item| {
        let (Some(set), Some(due)) = (item["set_unix_ms"].as_i64(), item["due_unix_ms"].as_i64())
        else {
            return false;
        };
        item["kind"] == "timer"
            && item["state"] == "scheduled"
            && item["snoozed"] == false
            && item["delivery"] == 1
            && (0..=SAME_SET_WITHIN_MS).contains(&now_ms.saturating_sub(set))
            && due.saturating_sub(set) == length
            && item["text"].as_str().is_some_and(|t| same_words(t, text))
    })?;
    let id = item["id"].as_u64().filter(|id| *id > 0)?;
    let (due, local) = resolved(item).ok()?;
    Some(
        json!({"status":"already_set","id":id,"delivery":1,"due_unix_ms":due,"local":local,"clock":clock_of(local),"message":"This exact timer was set a moment ago, so nothing new was set: there is one, not two. Say its time as `clock`, the local HH:MM it is due."}),
    )
}

/// The same words, as a person hears them: case and spacing aside.
fn same_words(a: &str, b: &str) -> bool {
    let words = |text: &str| {
        text.split_whitespace()
            .map(str::to_lowercase)
            .collect::<Vec<_>>()
    };
    words(a) == words(b)
}

/// Told beside a set when the line already has another timer or reminder
/// with the same words, not yet due (29e7, 2 Nov: a five-minute and a
/// two-minute water timer, both left set).
pub const SAME_WORDS: &str = "Another timer or reminder with the same words is still set (same_words_set). If the new one replaces it, cancel the old one by its id now; if both are wanted, say there are two.";

/// The items in a `list_set` page with the same words as `text`, set and
/// not yet due, as `{id, kind, clock}`; none from a reply without items.
fn same_words_set(page: &Value, text: &str) -> Vec<Value> {
    page["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| {
            item["state"] != "pending" && item["text"].as_str().is_some_and(|t| same_words(t, text))
        })
        .filter_map(|item| {
            Some(
                json!({"id":item["id"].as_u64()?,"kind":item["kind"].as_str()?,
                "clock":item["local"].as_str().and_then(clock_of)}),
            )
        })
        .collect()
}

/// A fresh compare-and-set guard on the store's identity and revision.
fn guard(store_id: &str, revision: u64) -> Result<Guard, Failure> {
    let mut random = [0u8; 16];
    getrandom::fill(&mut random).map_err(|_| Failure::Unavailable)?;
    Ok(Guard {
        store_id: store_id.into(),
        revision,
        request_id: random.iter().map(|b| format!("{b:02x}")).collect(),
    })
}

#[derive(Clone, Copy, Debug)]
enum Failure {
    BadArguments,
    Unavailable,
    Uncertain,
    Conflict,
    NotFound,
    NotPending,
    BadTime,
    Full,
}

fn failure(why: Failure) -> Value {
    let (code, text) = match why {
        Failure::BadArguments => (
            "bad_arguments",
            "I need a valid reminder label, time and exact arguments before I can do that.",
        ),
        Failure::Unavailable => (
            "unavailable",
            "I cannot use my reminder store just now. Please try reading the current reminders later.",
        ),
        Failure::Uncertain => (
            "uncertain",
            "I could not confirm that change. It may have happened. Please list the current reminders before asking me to make the change again.",
        ),
        Failure::Conflict => (
            "conflict",
            "The reminders changed while I was checking. Please list the current reminders before choosing that change again.",
        ),
        Failure::NotFound => ("not_found", "That reminder or timer is no longer active."),
        Failure::NotPending => (
            "not_pending",
            "That exact reminder is not currently due. Please list the current reminders before choosing one to snooze.",
        ),
        Failure::BadTime => (
            "bad_time",
            "I cannot set that time just now. Please clarify the date, time zone or repeated clock time, and check that my clock is ready.",
        ),
        Failure::Full => (
            "full",
            "My reminder store is full. Please list the current reminders and cancel one before adding another.",
        ),
    };
    json!({"ok":false,"error":text,"code":code})
}

fn invalid_reply(mutation: bool) -> Failure {
    if mutation {
        Failure::Uncertain
    } else {
        Failure::Unavailable
    }
}

fn refused(reply: &Value, mutation: bool) -> Failure {
    match reply["error"].as_str() {
        Some("conflict" | "bad_guard") => Failure::Conflict,
        Some("not_found") => Failure::NotFound,
        Some("stale_delivery") => Failure::NotPending,
        Some("bad_time") => Failure::BadTime,
        Some("full") => Failure::Full,
        Some("bad_request" | "bad_text" | "too_long" | "incomplete" | "busy" | "unavailable") => {
            Failure::Unavailable
        }
        // In particular storage errors can occur after commit; timeout or an
        // unrecognized error never proves a mutation did not take effect.
        _ => invalid_reply(mutation),
    }
}

enum Call {
    Set {
        text: String,
        when: When,
    },
    List {
        after_id: u64,
        pending_only: bool,
    },
    Cancel {
        id: u64,
    },
    /// Cancel the one item whose text holds these words ([`by_text`]).
    CancelText {
        text: String,
    },
    Snooze {
        id: u64,
        seconds: u64,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Timer {
    text: String,
    seconds: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reminder {
    text: String,
    time: String,
    day: Option<String>,
    zone: Option<String>,
    fold: Option<Fold>,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Every {
    Daily,
    Weekly,
    /// Every so many minutes or hours ([`EVERY_SWITCH`]).
    Interval,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Repeating {
    text: String,
    #[serde(default)]
    time: Option<String>,
    every: Every,
    weekday: Option<Weekday>,
    zone: Option<String>,
    #[serde(default)]
    minutes: Option<u32>,
    #[serde(default)]
    hours: Option<u32>,
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Listing {
    #[serde(default)]
    after_id: u64,
    #[serde(default)]
    pending_only: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Cancel {
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    text: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Snooze {
    id: u64,
    seconds: u64,
}

fn decode<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T, Failure> {
    if value
        .as_object()
        .is_none_or(|fields| fields.values().any(Value::is_null))
    {
        return Err(Failure::BadArguments);
    }
    serde_json::from_value(value.clone()).map_err(|_| Failure::BadArguments)
}
/// At most this many list pages are read to find a cancel's text.
const MOST_TEXT_PAGES: usize = 8;

/// Whether a listed item's text holds `words`, case and edge spaces aside:
/// "stop reminding me to drink water" with `drink water` finds "Drink water".
fn by_text(item: &Value, words: &str) -> bool {
    let words = words.trim().to_lowercase();
    !words.is_empty()
        && item["text"]
            .as_str()
            .is_some_and(|text| text.to_lowercase().contains(&words))
}

fn valid_text(text: &str) -> bool {
    !text.trim().is_empty() && text.len() <= 300 && !text.chars().any(char::is_control)
}
fn seconds(value: u64) -> Result<(), Failure> {
    (1..=MAX_AFTER_SECONDS)
        .contains(&value)
        .then_some(())
        .ok_or(Failure::BadArguments)
}
fn time(value: &str) -> bool {
    let b = value.as_bytes();
    b.len() == 5
        && b[2] == b':'
        && [b[0], b[1], b[3], b[4]].iter().all(u8::is_ascii_digit)
        && &b[..2] <= b"23"
        && &b[3..] <= b"59"
}
fn day(value: &str) -> bool {
    if matches!(value, "today" | "tomorrow" | "next") {
        return true;
    }
    let b = value.as_bytes();
    if b.len() != 10
        || b[4] != b'-'
        || b[7] != b'-'
        || b.iter()
            .enumerate()
            .any(|(i, b)| i != 4 && i != 7 && !b.is_ascii_digit())
    {
        return false;
    }
    let year: u16 = value[..4].parse().unwrap_or(0);
    let month: usize = value[5..7].parse().unwrap_or(0);
    let day: u8 = value[8..].parse().unwrap_or(0);
    if !(1970..=9999).contains(&year) || !(1..=12).contains(&month) {
        return false;
    }
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let max = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ][month - 1];
    (1..=max).contains(&day)
}
fn calendar(time_of_day: &str, zone: Option<&str>) -> Result<(), Failure> {
    if !time(time_of_day) || zone.is_some_and(|z| validate_zone(z).is_err()) {
        return Err(Failure::BadArguments);
    }
    Ok(())
}
/// `every`: [`EVERY_SWITCH`] is on, so `repeat_reminder` takes `interval`.
fn parse(name: &str, value: &Value, every: bool) -> Result<Call, Failure> {
    let call = match name {
        "set_timer" => {
            let a: Timer = decode(value)?;
            seconds(a.seconds)?;
            Call::Set {
                text: a.text,
                when: When::After { seconds: a.seconds },
            }
        }
        "set_reminder" => {
            let a: Reminder = decode(value)?;
            calendar(&a.time, a.zone.as_deref())?;
            if a.day.as_deref().is_some_and(|d| !day(d)) {
                return Err(Failure::BadArguments);
            }
            Call::Set {
                text: a.text,
                when: When::At {
                    time: a.time,
                    day: a.day,
                    zone: a.zone,
                    fold: a.fold,
                },
            }
        }
        "repeat_reminder" => {
            let a: Repeating = decode(value)?;
            let when = match (a.every, a.weekday) {
                (Every::Interval, None) if every => {
                    let window = match (a.from, a.to) {
                        (Some(from), Some(to)) if time(&from) && time(&to) => {
                            (Some(from), Some(to))
                        }
                        (None, None) => (None, None),
                        _ => return Err(Failure::BadArguments),
                    };
                    let step = match (a.minutes, a.hours) {
                        (Some(minutes), None) => Some(minutes),
                        (None, Some(hours)) => hours.checked_mul(60),
                        _ => None,
                    };
                    if a.time.is_some()
                        || !step.is_some_and(|step| {
                            (MIN_EVERY_MINUTES..=MAX_EVERY_MINUTES).contains(&step)
                        })
                        || a.zone.as_deref().is_some_and(|z| validate_zone(z).is_err())
                    {
                        return Err(Failure::BadArguments);
                    }
                    When::Every {
                        minutes: a.minutes,
                        hours: a.hours,
                        from: window.0,
                        to: window.1,
                        zone: a.zone,
                    }
                }
                (Every::Interval, _) => return Err(Failure::BadArguments),
                (every, weekday) => {
                    if a.minutes.is_some()
                        || a.hours.is_some()
                        || a.from.is_some()
                        || a.to.is_some()
                    {
                        return Err(Failure::BadArguments);
                    }
                    let time_of_day = a.time.ok_or(Failure::BadArguments)?;
                    calendar(&time_of_day, a.zone.as_deref())?;
                    match (every, weekday) {
                        (Every::Daily, None) => When::Daily {
                            time: time_of_day,
                            zone: a.zone,
                        },
                        (Every::Weekly, Some(weekday)) => When::Weekly {
                            weekday,
                            time: time_of_day,
                            zone: a.zone,
                        },
                        _ => return Err(Failure::BadArguments),
                    }
                }
            };
            Call::Set { text: a.text, when }
        }
        "list_reminders" => {
            let a: Listing = decode(value)?;
            Call::List {
                after_id: a.after_id,
                pending_only: a.pending_only,
            }
        }
        "cancel_reminder" => {
            let a: Cancel = decode(value)?;
            match (a.id, a.text) {
                (Some(0), _) | (Some(_), Some(_)) | (None, None) => {
                    return Err(Failure::BadArguments);
                }
                (Some(id), None) => Call::Cancel { id },
                (None, Some(text)) if valid_text(&text) => Call::CancelText { text },
                (None, Some(_)) => return Err(Failure::BadArguments),
            }
        }
        "snooze_reminder" => {
            let a: Snooze = decode(value)?;
            seconds(a.seconds)?;
            if a.id == 0 {
                return Err(Failure::BadArguments);
            }
            Call::Snooze {
                id: a.id,
                seconds: a.seconds,
            }
        }
        _ => return Err(Failure::BadArguments),
    };
    if matches!(&call, Call::Set { text, .. } if !valid_text(text)) {
        return Err(Failure::BadArguments);
    }
    Ok(call)
}

fn identity(reply: &Value) -> Result<(&str, u64), Failure> {
    let store = reply["store_id"]
        .as_str()
        .filter(|s| {
            s.len() == 32
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
        .ok_or(Failure::Unavailable)?;
    let revision = reply["revision"].as_u64().ok_or(Failure::Unavailable)?;
    Ok((store, revision))
}
/// The due time's local wall clock, `HH:MM`, from `local` (RFC 9557,
/// "2026-10-25T21:07:34.939+07:00[Asia/Ho_Chi_Minh]"): the time the model
/// says. On lamp-4ace on 25 Oct it read "07:34" out of such a string and
/// said "seven thirty-four" for a reminder due at 21:07, and "eight
/// forty-seven" for a timer due at 21:08.
pub fn clock_of(local: &str) -> Option<String> {
    let hm = local.split_once('T')?.1.get(..5)?;
    let b = hm.as_bytes();
    (b[2] == b':' && b[..2].iter().all(u8::is_ascii_digit) && b[3..].iter().all(u8::is_ascii_digit))
        .then(|| hm.to_owned())
}
fn resolved(reply: &Value) -> Result<(i64, &str), Failure> {
    let due = reply["due_unix_ms"].as_i64().ok_or(Failure::Uncertain)?;
    let local = reply["local"].as_str().ok_or(Failure::Uncertain)?;
    Deadline {
        unix_ms: due,
        elapsed: None,
        repeat: None,
        local: local.into(),
    }
    .check()
    .map_err(|_| Failure::Uncertain)?;
    Ok((due, local))
}
fn committed(request: &Request, reply: &Value) -> Result<Value, Failure> {
    let guard = request.guard().ok_or(Failure::Uncertain)?;
    let (store, revision) = identity(reply).map_err(|_| Failure::Uncertain)?;
    if store != guard.store_id || guard.revision.checked_add(1) != Some(revision) {
        return Err(Failure::Uncertain);
    }
    let id = reply["id"]
        .as_u64()
        .filter(|n| *n > 0)
        .ok_or(Failure::Uncertain)?;
    let delivery = reply["delivery"]
        .as_u64()
        .filter(|n| *n > 0)
        .ok_or(Failure::Uncertain)?;
    match request {
        Request::Set { .. } if delivery == 1 => {
            let (due, local) = resolved(reply)?;
            Ok(
                json!({"status":"committed","id":id,"delivery":delivery,"due_unix_ms":due,"local":local,"clock":clock_of(local),"message":"The timer or reminder is saved. Say its time as `clock`, the local HH:MM it is due, never read from `local`."}),
            )
        }
        Request::Cancel { id: asked, .. } if *asked == id && reply["cancelled"] == true => Ok(
            json!({"status":"committed","id":id,"cancelled":true,"message":"The selected timer or whole reminder series is cancelled."}),
        ),
        Request::Snooze {
            id: asked,
            delivery: old,
            ..
        } if *asked == id && old.checked_add(1) == Some(delivery) && reply["snoozed"] == true => {
            let (due, local) = resolved(reply)?;
            Ok(
                json!({"status":"committed","id":id,"delivery":delivery,"snoozed":true,"due_unix_ms":due,"local":local,"clock":clock_of(local),"message":"The current occurrence is postponed; its repeating schedule is unchanged. Say its new time as `clock`, the local HH:MM, never read from `local`."}),
            )
        }
        _ => Err(Failure::Uncertain),
    }
}

#[derive(Deserialize)]
struct Item {
    id: u64,
    delivery: u64,
    text: String,
    kind: String,
    state: String,
    due_unix_ms: i64,
    local: String,
    repeat: Option<Repeat>,
    snoozed: bool,
    late: bool,
    /// When it was set, in the lamp's zone (`list_set`): absent from an
    /// older Schedule service, null when it can't be said ([`set_at`]).
    #[serde(default)]
    set_local: Option<String>,
    #[serde(default)]
    set_day: Option<String>,
}
/// When an item was set, as the model is told it: its day (`today`,
/// `yesterday` or `YYYY-MM-DD`) and its local `HH:MM`, both or neither. An
/// item without them (an older Schedule, a zone it can't name) is told as
/// set at an unknown time, never guessed.
fn set_at(item: &Item) -> (Option<String>, Option<String>) {
    let day = item
        .set_day
        .as_deref()
        .filter(|d| matches!(*d, "today" | "yesterday") || (d.len() == 10 && day(d)));
    match (day, item.set_local.as_deref().and_then(clock_of)) {
        (Some(day), Some(clock)) => (Some(day.to_owned()), Some(clock)),
        _ => (None, None),
    }
}
fn item(value: &Value) -> Result<Item, Failure> {
    if value.get("repeat").is_none() {
        return Err(Failure::Unavailable);
    }
    let item: Item = serde_json::from_value(value.clone()).map_err(|_| Failure::Unavailable)?;
    if item.id == 0
        || item.delivery == 0
        || !valid_text(&item.text)
        || !matches!(item.kind.as_str(), "timer" | "reminder")
        || !matches!(
            item.state.as_str(),
            "pending" | "scheduled" | "waiting_for_clock"
        )
    {
        return Err(Failure::Unavailable);
    }
    resolved(value).map_err(|_| Failure::Unavailable)?;
    if let Some(repeat) = &item.repeat {
        let (time_of_day, zone, date) = match repeat {
            Repeat::Daily { time, zone, date }
            | Repeat::Weekly {
                time, zone, date, ..
            }
            | Repeat::Every {
                time, zone, date, ..
            } => (time, zone, date),
        };
        if !time(time_of_day) || validate_zone(zone).is_err() || date.len() != 10 || !day(date) {
            return Err(Failure::Unavailable);
        }
        if let Repeat::Every {
            minutes, from, to, ..
        } = repeat
            && (!(MIN_EVERY_MINUTES..=MAX_EVERY_MINUTES).contains(minutes)
                || from.is_some() != to.is_some()
                || from.as_deref().is_some_and(|t| !time(t))
                || to.as_deref().is_some_and(|t| !time(t)))
        {
            return Err(Failure::Unavailable);
        }
    }
    Ok(item)
}
/// Room kept in a probe for what is added after the page: the count of what
/// is not yet due (#1595), and of what is someone's own ([`HIDDEN`]).
fn reserve(probe: &mut Value, pending_only: bool, hidden: bool) {
    if pending_only {
        probe["not_yet_due"] = json!(u64::MAX);
        probe["next"] = json!(NOT_YET_DUE);
    }
    if hidden {
        probe["hidden"] = json!(u64::MAX);
        probe["hidden_note"] = json!(HIDDEN);
    }
}

fn page(
    reply: &Value,
    after_id: u64,
    pending_only: bool,
    set_times: bool,
    hidden: bool,
) -> Result<Value, Failure> {
    let values = reply["items"]
        .as_array()
        .filter(|v| v.len() <= 8)
        .ok_or(Failure::Unavailable)?;
    let server_next = match reply.get("next_after_id") {
        Some(Value::Null) => None,
        Some(v) => Some(v.as_u64().ok_or(Failure::Unavailable)?),
        None => return Err(Failure::Unavailable),
    };
    let mut checked = Vec::new();
    let mut previous = after_id;
    for value in values {
        let i = item(value)?;
        if i.id <= previous || (pending_only && i.state != "pending") {
            return Err(Failure::Unavailable);
        }
        previous = i.id;
        let mut one = json!({"id":i.id,"kind":i.kind,"text":i.text,"state":i.state,"local":i.local,"clock":clock_of(&i.local),"due_unix_ms":i.due_unix_ms,"repeat":i.repeat,"snoozed":i.snoozed,"late":i.late});
        if set_times {
            let (set_day, set_clock) = set_at(&i);
            one["set_day"] = json!(set_day);
            one["set_clock"] = json!(set_clock);
        }
        checked.push(one);
    }
    if server_next.is_some_and(|n| values.is_empty() || n != previous) {
        return Err(Failure::Unavailable);
    }
    let mut items = Vec::new();
    for value in &checked {
        items.push(value.clone());
        let mut probe = json!({"items":items,"next_after_id":u64::MAX,"pending_only":pending_only,"complete":false});
        reserve(&mut probe, pending_only, hidden);
        if probe.to_string().len() > MAX_RESULT_BYTES {
            items.pop();
            break;
        }
    }
    if items.is_empty() && !checked.is_empty() {
        return Err(Failure::Unavailable);
    }
    let next = if items.len() < checked.len() {
        items.last().and_then(|i| i["id"].as_u64())
    } else {
        server_next
    };
    let mut listed = json!({"items":items,"next_after_id":next,"pending_only":pending_only,"complete":next.is_none()});
    if set_times && let Some(said) = reply.get("said") {
        said_recently(&mut listed, said, pending_only, hidden)?;
    }
    Ok(listed)
}

/// The reminders already said (`list_set`'s `said`, 29e7, 2 Nov), as the
/// model is told them: newest first, each with when it was due and said,
/// as a day and a local `HH:MM`, whether on time, and whether it waits for
/// an answer; as many as fit, with `said_complete` false when some don't.
fn said_recently(
    listed: &mut Value,
    said: &Value,
    pending_only: bool,
    hidden: bool,
) -> Result<(), Failure> {
    let said = said
        .as_array()
        .filter(|said| said.len() <= lamp_schedule::engine::SAID_KEPT)
        .ok_or(Failure::Unavailable)?;
    let day_of = |value: &Value| {
        value
            .as_str()
            .filter(|d| matches!(*d, "today" | "yesterday") || (d.len() == 10 && day(d)))
            .map(str::to_owned)
    };
    let mut views = Vec::new();
    for one in said {
        let text = one["text"]
            .as_str()
            .filter(|t| valid_text(t))
            .ok_or(Failure::Unavailable)?;
        let kind = one["kind"]
            .as_str()
            .filter(|k| matches!(*k, "timer" | "reminder"))
            .ok_or(Failure::Unavailable)?;
        let (Some(on_time), Some(waiting)) = (
            one["on_time"].as_bool(),
            one["waiting_for_answer"].as_bool(),
        ) else {
            return Err(Failure::Unavailable);
        };
        let clock = |field: &str| one[field].as_str().and_then(clock_of);
        let (said_day, said_clock) = match (day_of(&one["said_day"]), clock("said_local")) {
            (Some(day), Some(clock)) => (Some(day), Some(clock)),
            _ => (None, None),
        };
        let (due_day, due_clock) = match (day_of(&one["due_day"]), clock("due_local")) {
            (Some(day), Some(clock)) => (Some(day), Some(clock)),
            _ => (None, None),
        };
        views.push(json!({"text":text,"kind":kind,"said_day":said_day,"said_clock":said_clock,
            "due_day":due_day,"due_clock":due_clock,"on_time":on_time,"waiting_for_answer":waiting}));
    }
    let mut kept = Vec::new();
    for view in &views {
        kept.push(view.clone());
        let mut probe = listed.clone();
        probe["said_recently"] = json!(kept);
        probe["said_complete"] = json!(false);
        reserve(&mut probe, pending_only, hidden);
        if probe.to_string().len() > MAX_RESULT_BYTES {
            kept.pop();
            break;
        }
    }
    listed["said_complete"] = json!(kept.len() == views.len());
    listed["said_recently"] = json!(kept);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repeat(args: Value, every: bool) -> Result<When, Failure> {
        match parse("repeat_reminder", &args, every)? {
            Call::Set { when, .. } => Ok(when),
            _ => Err(Failure::BadArguments),
        }
    }

    /// `LAMP_SCHEDULE_EVERY` off: `repeat_reminder` is the catalog it was and
    /// refuses an interval; on, it takes minutes or hours, 15 minutes to 24
    /// hours, never both, with an optional window of both ends.
    #[test]
    fn interval_repeats_only_with_the_switch_and_within_its_bounds() {
        let off = catalog_with(false);
        assert_eq!(
            off[2].parameters.as_ref().unwrap()["properties"]["every"]["enum"],
            json!(["daily", "weekly"])
        );
        assert!(off[2].parameters.as_ref().unwrap()["properties"]["minutes"].is_null());
        let on = catalog_with(true);
        assert_eq!(
            on[2].parameters.as_ref().unwrap()["properties"]["every"]["enum"],
            json!(["daily", "weekly", "interval"])
        );
        let hourly = json!({"text":"Drink some water","every":"interval","hours":1});
        assert!(repeat(hourly.clone(), false).is_err());
        assert_eq!(
            repeat(hourly, true).unwrap(),
            When::Every {
                minutes: None,
                hours: Some(1),
                from: None,
                to: None,
                zone: None
            }
        );
        let windowed =
            json!({"text":"Water","every":"interval","minutes":90,"from":"22:00","to":"02:00"});
        assert!(matches!(
            repeat(windowed, true).unwrap(),
            When::Every {
                minutes: Some(90),
                from: Some(_),
                to: Some(_),
                ..
            }
        ));
        for wrong in [
            json!({"text":"t","every":"interval","minutes":14}),
            json!({"text":"t","every":"interval","hours":25}),
            json!({"text":"t","every":"interval","minutes":60,"hours":1}),
            json!({"text":"t","every":"interval"}),
            json!({"text":"t","every":"interval","minutes":60,"from":"08:00"}),
            json!({"text":"t","every":"interval","minutes":60,"time":"09:00"}),
            json!({"text":"t","every":"daily","time":"09:00","minutes":60}),
        ] {
            assert!(repeat(wrong.clone(), true).is_err(), "{wrong}");
        }
        // Daily and weekly are as they were, either way.
        for every in [false, true] {
            assert!(repeat(json!({"text":"t","every":"daily","time":"09:00"}), every).is_ok());
            assert!(repeat(json!({"text":"t","every":"daily"}), every).is_err());
        }
    }
}
