//! A spoken line goes only into a session opened for the person it was
//! admitted for (#1675, the first builder at 17:00 on 19 October), off
//! unless `LAMP_LIVE_SCOPED_SESSION=on`. The Live route only.
//!
//! Each Live session is one person's (`memory.md`, "A session for each
//! person"), but whose it is follows the counted person, which waits 10 s
//! before someone else known counts. A line admitted meanwhile for B, by
//! the face the eyes named as it began (#1794's `memory_scope`), went into
//! A's session, which already holds A's kept things, thread and
//! conversation.
//!
//! With the switch, such a line is not sent: it fails as not delivered,
//! and the session is opened again, fresh, for B between turns. B's next
//! line is answered in B's own session. The cost is that first line.
//! Two known people taking turns share a room session, so after the
//! second one's first line no line is dropped at a change of speaker.
//! A line admitted for the room, a room session, and written requests and
//! reminders go on as before: a room session holds nobody's private
//! context, and a known person who keeps talking turned away from the eyes
//! keeps their session (`thread::QUIET_BEFORE_ROOM`).

/// Where the coordinator reads the switch.
pub const ENVIRONMENT: &str = "LAMP_LIVE_SCOPED_SESSION";
/// The provider worker's flag for it, right after `--messages`.
pub const FLAG: &str = "--live-scoped-session";

/// The worker's flags for the switch, from the coordinator's environment.
pub fn arguments_from_environment() -> Vec<String> {
    arguments(std::env::var(ENVIRONMENT).ok().as_deref())
}

fn arguments(value: Option<&str>) -> Vec<String> {
    if value == Some("on") {
        vec![FLAG.to_owned()]
    } else {
        Vec::new()
    }
}

/// Take a leading [`FLAG`] off a worker's flags.
pub fn take(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == FLAG => (true, rest),
        _ => (false, flags),
    }
}

/// Whether a line admitted for `admitted` must not go into a session opened
/// for `opened_for`: only when both are known people and not the same one.
pub fn refuses(admitted: Option<&str>, opened_for: Option<&str>) -> bool {
    matches!((admitted, opened_for), (Some(admitted), Some(opened)) if admitted != opened)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_is_on_only_for_on_and_its_flag_is_taken_first() {
        assert_eq!(arguments(Some("on")), [FLAG]);
        for off in [None, Some(""), Some("off"), Some("1"), Some("ON")] {
            assert!(arguments(off).is_empty(), "{off:?}");
        }
        let flags = [FLAG.to_owned(), "x".to_owned()];
        assert_eq!(take(&flags), (true, &flags[1..]));
        let other = ["x".to_owned()];
        assert_eq!(take(&other), (false, &other[..]));
    }

    #[test]
    fn only_a_known_person_in_another_known_persons_session_is_refused() {
        assert!(refuses(Some("p-b"), Some("p-a")));
        assert!(!refuses(Some("p-a"), Some("p-a")));
        assert!(!refuses(None, Some("p-a")));
        assert!(!refuses(Some("p-b"), None));
        assert!(!refuses(None, None));
    }
}
