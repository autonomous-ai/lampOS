//! A guest's words in the room's memory (`LAMP_MEMORY_GUEST_MARKED`,
//! #8639, the memory owner's calls of 27 Nov 2026, 17:42 and 17:58).
//!
//! A *guest line* is a heard line admitted as the room's with the camera
//! allowed, an unknown face in view at the last refresh, no known face seen
//! within the last two minutes (`memory_scope::NO_FACE_QUIET`) and no
//! session person ([`crate::memory_scope::FaceCache::guest`]). Nobody in
//! view, a known face in view, or a one-turn misread (a known face seen
//! moments before) is never one: the line stays the owner's, as today.
//!
//! A guest's words are kept in the room's memory, as today, and now marked
//! as a guest's: the room's `append_linked` and `remember` carry
//! `"speaker":"guest"` ([`lamp_msgs::known::GUEST_SPEAKER`]), kept as a row
//! of the memory's `speakers` table. Recall tells a marked line
//! `"said_by":"a guest"` and takes `guest: true` for only what a guest said,
//! so the owner or an enrolled user can ask "what did my guest say?". A
//! guest's kept fact is told "A guest said: …" in the session note and the
//! hits, never as the household's, and expires after 30 days, as their
//! exchanges do. The private room's service records, reminders and app
//! lines are as today, and a guest still hears the household's room notes.
//! Off, nothing is marked and every word is as before.

/// Only the exact value `on`; off unless set.
pub const ENVIRONMENT: &str = "LAMP_MEMORY_GUEST_MARKED";

/// How many recent guest lines are held for the checks given only a
/// request id ([`line`]): bookkeeping, read as the line's exchange is
/// queued, right after its reply.
const LINES_HELD: usize = 16;

#[cfg(test)]
thread_local! {
    static ON_FOR_TEST: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
    static MARKS_FOR_TEST: std::cell::RefCell<Marks> = const { std::cell::RefCell::new(Marks::new()) };
}

/// Whether the switch is on.
pub fn on() -> bool {
    #[cfg(test)]
    if let Some(on) = ON_FOR_TEST.with(std::cell::Cell::get) {
        return on;
    }
    on_for(std::env::var(ENVIRONMENT).ok().as_deref())
}

fn on_for(value: Option<&str>) -> bool {
    value == Some("on")
}

/// This thread's [`ENVIRONMENT`], for a test; `None` reads the environment
/// again (and forgets this thread's marks).
#[cfg(test)]
pub(crate) fn set_on_for_test(on: Option<bool>) {
    ON_FOR_TEST.with(|cell| cell.set(on));
    if on.is_none() {
        MARKS_FOR_TEST.with(|marks| *marks.borrow_mut() = Marks::new());
    }
}

/// The newest [`LINES_HELD`] requests admitted as guest lines.
struct Marks {
    guests: [u64; LINES_HELD],
    next: usize,
}

impl Marks {
    const fn new() -> Self {
        Self {
            guests: [0; LINES_HELD],
            next: 0,
        }
    }

    fn mark(&mut self, request: u64, guest: bool) {
        if guest {
            self.guests[self.next] = request;
            self.next = (self.next + 1) % LINES_HELD;
        } else {
            for held in self.guests.iter_mut().filter(|held| **held == request) {
                *held = 0;
            }
        }
    }

    fn is(&self, request: u64) -> bool {
        request != 0 && self.guests.contains(&request)
    }
}

#[cfg(not(test))]
static MARKS: std::sync::Mutex<Marks> = std::sync::Mutex::new(Marks::new());

fn with_marks<T>(read: impl FnOnce(&mut Marks) -> T) -> T {
    #[cfg(test)]
    {
        MARKS_FOR_TEST.with(|marks| read(&mut marks.borrow_mut()))
    }
    #[cfg(not(test))]
    {
        read(&mut MARKS.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

/// The second route admitted `request`, a guest line or not (as the face
/// cache read it at admission). Kept whatever the switch, read only with
/// it on.
pub fn mark_line(request: u64, guest: bool) {
    with_marks(|marks| marks.mark(request, guest));
}

/// Whether `request` was admitted as a guest line, with the switch on.
pub fn line(request: u64) -> bool {
    on() && with_marks(|marks| marks.is(request))
}

/// The journal's words beside a room line's reason, for a guest line with
/// the switch on: no name, nothing but the mark.
pub fn journal(guest: bool) -> &'static str {
    if guest && on() {
        ", a guest: marked for the room's memory (LAMP_MEMORY_GUEST_MARKED)"
    } else {
        ""
    }
}

/// What a recall item a guest said carries (W1, approved 27 Nov 17:58).
pub const SAID_BY: &str = "a guest";

/// W1's rule, after the recall answer's `scope`, when any item says
/// [`SAID_BY`].
pub const SAID_BY_RULE: &str = " Items with said_by were said by a guest, someone the lamp did not know: tell them as the guest's, never as the asker's or the household's.";

/// W2: recall's `guest` parameter. W2's other half, "For what a guest
/// said, give guest true." in recall's description, is left out: under the
/// product's switches (`LAMP_MEMORY_RECALL_V2`, `LAMP_MEMORY_REPLACES`)
/// recall's description is 1,020 of a function's 1,024 bytes
/// (`Tool::validate`, `tools::admitted`), and with it 1,060, so recall would
/// be dropped. The parameter's own words say what it is for.
pub const RECALL_PARAMETER: &str =
    "true for only what a guest, someone the lamp did not know, said here.";

/// W3: before a guest's kept fact in the session note and the hits.
pub const SAID: &str = "A guest said: ";

/// Whether a memory entry is a guest's (`"said_by":"guest"`, the memory's
/// mark), with the switch on.
pub fn said(entry: &serde_json::Value) -> bool {
    on() && entry["said_by"] == lamp_msgs::known::GUEST_SPEAKER
}

/// The memory's functions with recall's `guest` (W2), with the switch on;
/// off, as they are.
pub(crate) fn described(mut tools: Vec<lamp_gemini::Tool>) -> Vec<lamp_gemini::Tool> {
    if !on() {
        return tools;
    }
    for tool in tools.iter_mut().filter(|tool| tool.name == super::RECALL) {
        if let Some(parameters) = tool.parameters.as_mut() {
            parameters["properties"]["guest"] =
                serde_json::json!({"type": "boolean", "description": RECALL_PARAMETER});
        }
    }
    tools
}

/// A validated room snapshot with each fact a guest said (the memory's
/// `guest_ids`) told as [`SAID`] its words (W3), with the switch on; off,
/// or with none, as it is.
pub(crate) fn told_in_note(snapshot: &serde_json::Value) -> Option<serde_json::Value> {
    if !on() {
        return None;
    }
    let guests: Vec<u64> = snapshot["guest_ids"]
        .as_array()?
        .iter()
        .filter_map(serde_json::Value::as_u64)
        .collect();
    let mut quoted: serde_json::Value = serde_json::from_str(snapshot["note"].as_str()?).ok()?;
    let mut told = 0;
    for fact in quoted.get_mut("facts")?.as_array_mut()? {
        if fact["id"].as_u64().is_some_and(|id| guests.contains(&id))
            && let Some(text) = fact["text"].as_str()
        {
            fact["text"] = serde_json::json!(format!("{SAID}{text}"));
            told += 1;
        }
    }
    if told == 0 {
        return None;
    }
    let mut snapshot = snapshot.clone();
    snapshot["note"] = serde_json::json!(quoted.to_string());
    Some(snapshot)
}

#[cfg(test)]
#[path = "guest_tests.rs"]
mod tests;
