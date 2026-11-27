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

/// The memory's functions with recall's `guest` (W2), with the switch on.
pub(crate) fn described(tools: Vec<lamp_gemini::Tool>) -> Vec<lamp_gemini::Tool> {
    tools
}

#[cfg(test)]
#[path = "guest_tests.rs"]
mod tests;
