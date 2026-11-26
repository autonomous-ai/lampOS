//! A visitor is never the owner (`LAMP_MEMORY_VISITOR_NOT_KEPT`, #8639's
//! common part). A *visitor line* is a heard line admitted as the room's
//! with the camera allowed, an unknown face in view at the last refresh,
//! and no known face seen in this conversation's time
//! (`memory_scope::RoomReason::NeverKnown`, within `NO_FACE_QUIET`), and
//! not carried or taken as anyone's since ([`ScopeLease::visitor`]).
//!
//! Under `LAMP_PRIVATE_ROOM` every line is the owner's for the services,
//! so a stranger alone at the lamp was told the owner's service records,
//! linked reads, app lines and reminders, and a reminder they set was kept
//! as the owner's. With this switch on, the private room yields to a
//! visitor line ([`private_room`]): it is judged as it would be outside the
//! private room, where a room line is nobody's, so nothing only the owner
//! may hear is told and nothing is kept as theirs. A line with nobody in
//! view, and one after a known face seen moments ago (a misread, the
//! owner's 23 Nov rule), are unchanged.
//!
//! Whether a visitor's own words are kept in the room's memory is the
//! memory owner's to rule (#8639, options A, B and C), and not changed
//! here. Off, everything is as it was.
use crate::memory_scope::ScopeLease;

/// Only the exact value `on`; off unless set.
pub const ENVIRONMENT: &str = "LAMP_MEMORY_VISITOR_NOT_KEPT";

/// How many recent requests' visitor marks are held for the checks that
/// are given only a request id ([`line`]).
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

/// The admitted lines' marks: the newest [`LINES_HELD`] requests that were
/// visitor lines, and whether the newest line admitted was one.
struct Marks {
    visitors: [u64; LINES_HELD],
    next: usize,
    latest: bool,
}

impl Marks {
    const fn new() -> Self {
        Self {
            visitors: [0; LINES_HELD],
            next: 0,
            latest: false,
        }
    }

    fn mark(&mut self, request: u64, visitor: bool) {
        if visitor {
            self.visitors[self.next] = request;
            self.next = (self.next + 1) % LINES_HELD;
        } else {
            for held in self.visitors.iter_mut().filter(|held| **held == request) {
                *held = 0;
            }
        }
        self.latest = visitor;
    }

    fn is(&self, request: u64) -> bool {
        request != 0 && self.visitors.contains(&request)
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

/// The second route admitted `request`, a visitor line or not (as the face
/// cache read it at admission, [`crate::memory_scope::FaceCache::visitor`]).
/// Kept whatever the switch, read only with it on.
pub fn mark_line(request: u64, visitor: bool) {
    with_marks(|marks| marks.mark(request, visitor));
}

/// Whether `request` was admitted as a visitor line, with the switch on:
/// for the checks given only a request id.
pub fn line(request: u64) -> bool {
    on() && with_marks(|marks| marks.is(request))
}

/// Whether the newest line admitted was a visitor's, with the switch on:
/// for the checks made with no line of their own (a run's write read back,
/// a made skill's code said back).
pub fn latest() -> bool {
    on() && with_marks(|marks| marks.latest)
}

/// Whether `lease`'s line is a visitor's, with the switch on.
pub fn of(lease: Option<&ScopeLease>) -> bool {
    on() && lease.is_some_and(ScopeLease::visitor)
}

/// The private room (`person::private_room`) for the line `lease`: off for
/// a visitor line with the switch on, so the line is judged as outside it.
pub fn private_room(lease: Option<&ScopeLease>) -> bool {
    crate::person::private_room() && !of(lease)
}

/// [`private_room`] for the line `request`, as [`line`] reads it.
pub fn private_room_for_request(request: u64) -> bool {
    crate::person::private_room() && !line(request)
}

/// [`private_room`] for a check with no line of its own, as [`latest`]
/// reads it.
pub fn private_room_now() -> bool {
    crate::person::private_room() && !latest()
}

/// The journal's words beside a room line's reason, for a visitor line
/// with the switch on: no name, nothing but the mark.
pub fn journal(visitor: bool) -> &'static str {
    if visitor && on() {
        ", a visitor: the private room yields (LAMP_MEMORY_VISITOR_NOT_KEPT)"
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_is_on_only_for_exactly_on() {
        assert!(on_for(Some("on")));
        for value in [
            None,
            Some(""),
            Some("off"),
            Some("ON"),
            Some("on "),
            Some("1"),
        ] {
            assert!(!on_for(value), "{value:?}");
        }
    }

    #[test]
    fn marks_are_read_only_with_the_switch_on() {
        set_on_for_test(Some(false));
        mark_line(8639, true);
        assert!(!line(8639));
        assert!(!latest());
        set_on_for_test(Some(true));
        assert!(line(8639));
        assert!(latest());
        mark_line(8640, false);
        assert!(line(8639), "a later line keeps an earlier visitor's mark");
        assert!(!line(8640));
        assert!(!latest(), "the newest line is not a visitor's");
        mark_line(8639, false);
        assert!(!line(8639), "a line marked again as not a visitor's");
        assert!(!line(0));
        set_on_for_test(None);
    }

    #[test]
    fn only_the_newest_lines_are_held() {
        set_on_for_test(Some(true));
        for request in 1..=(LINES_HELD as u64 + 1) {
            mark_line(request, true);
        }
        assert!(!line(1), "the oldest is let go");
        assert!(line(2) && line(LINES_HELD as u64 + 1));
        set_on_for_test(None);
    }

    #[test]
    fn the_private_room_yields_only_to_a_visitor_with_the_switch_on() {
        crate::person::set_private_room_for_test(Some(true));
        set_on_for_test(Some(true));
        mark_line(7, true);
        mark_line(8, false);
        assert!(!private_room_for_request(7));
        assert!(private_room_for_request(8));
        assert!(private_room_now(), "the newest line is not a visitor's");
        assert!(private_room(None), "no lease, no visitor");
        set_on_for_test(Some(false));
        assert!(private_room_for_request(7), "off: as stock");
        mark_line(9, true);
        assert!(private_room_now(), "off: as stock");
        assert_eq!(journal(true), "");
        set_on_for_test(None);
        crate::person::set_private_room_for_test(None);
    }
}
