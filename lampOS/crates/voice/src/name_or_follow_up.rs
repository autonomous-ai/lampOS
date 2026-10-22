//! With people talking at a desk, a line is the lamp's only by its name, a
//! tap, its words calling the lamp, or a follow-up said soon after the lamp
//! spoke: off unless `LAMP_NAME_OR_FOLLOW_UP=on`, on both voice routes.
//!
//! Why: at the owner's desk on lamp-4ace on 22 October, the owner talked
//! to a colleague and the lamp kept answering. Of its 18 answers, its
//! journal gave 8 to "someone is turned to the lamp", 6 to "a conversation
//! with the lamp is going on", and 4 to its name. A person at a desk faces
//! the lamp's way while talking to someone else, so a face turned to the
//! lamp is no sign a line is for it, and this was never checked with real
//! people (`docs/attention.md`). The room was never judged busy either,
//! since the lamp answered everything, so the weak reasons ("the room is
//! not known to be busy", "the session has heard enough to be trusted")
//! were let out with the turned face as their sign. And each answer opened
//! a window (the thread's 45 s, 20 s after the lamp's words, a busy room's
//! 15 s) that let the next lines of the desk talk in.
//!
//! With the switch, on Live and on the second route, in a busy room
//! (`room_told::Room::Busy`):
//! - A face turned to the lamp is never a reason on its own
//!   (`attention::Attention::face_shows`). It still confirms the lamp's name
//!   heard: `LAMP_NAME_SECOND_SIGN`'s second sign, the strict busy form's
//!   name heard with someone turned, and a short sound with the name.
//! - A line is in the conversation going on when it began while the lamp's
//!   own words played or within [`FOLLOW_UP_FOR`] of their end, however long
//!   it then goes on, and nobody has talked past the lamp since; on the
//!   second route a call let out carries the next lines that long too. The
//!   windows read as the line is judged (the thread's 45 s, a busy room's
//!   15 s) don't decide: on lamp-52e6 on 22 October, follow-ups begun about
//!   7 s after their asks, as the answers played, went unanswered. A line
//!   begun later needs the name, a tap or its words calling the lamp. What
//!   the model is told of the conversation is not changed.
//!
//! In a room not known to be busy the lamp is as without the switch: a face
//! turned to it is a reason, and the conversation keeps its windows. The
//! owner, at 18:0x the same day with the switch on: "That's so weird if I
//! had to say lamp a thousand times throughout the day." A person alone with
//! the lamp does not say its name; one says it when others are around. So
//! the room must turn busy from the talk the lamp holds
//! (`room_told::RoomTold::count_held_talk`): in a room not known to be busy,
//! a line held as said to someone else by name, or as said to nobody with
//! someone in view and turned away (`bus_out::someone_in_view_turned_away`),
//! is talk even with only talk counted, never a lone voice's lines held with
//! nobody in view (on lamp-4ace on 22 October an unseen person's misheard
//! call and the lines after it turned the room busy, and the scene was
//! lost); and two people seen or heard (two faces in the eyes'
//! newest look, `bus_out::faces_in_view`, or a line in another voice than
//! the conversation's) make the room busy at once
//! (`room_told::RoomTold::two_people`). Its costs: a colleague off camera,
//! with no voice model, is answered while the person faces the lamp, since
//! nothing is held, until a line is held; and a face on a screen or in a
//! photo keeps the room busy, so the name is needed.
//!
//! Everything else is as without it: the name, a tap, the text calling the
//! lamp, said close to it, an answer to its own question inside the window,
//! its news, and the privacy and policy holds.

use std::time::Duration;

/// Where the coordinator reads the switch.
pub const ENVIRONMENT: &str = "LAMP_NAME_OR_FOLLOW_UP";
/// The provider worker's flag for it, right after `--live-scoped-session`,
/// read by both routes' workers.
pub const FLAG: &str = "--name-or-follow-up";

/// How long after the end of the lamp's own last words a line is still a
/// follow-up, let out as the conversation going on. Long enough for "And
/// tomorrow?" after an answer; short of the next lines of two people
/// talking at a desk after it.
// ponytail: chosen, not measured; set it again from a real desk-talk set.
pub const FOLLOW_UP_FOR: Duration = Duration::from_secs(8);

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

/// Whether a line said `ago` after the lamp's own last words ended (zero
/// while they still play; `None`: it said none) is a follow-up to them.
pub fn follows_up(ago: Option<Duration>) -> bool {
    ago.is_some_and(|ago| ago <= FOLLOW_UP_FOR)
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
        assert_eq!(take(&flags[1..]), (false, &flags[1..]));
    }

    /// A follow-up 5 s after the lamp's words is one, and one while they
    /// play; one 12 s after, or with no words of the lamp's, is not.
    #[test]
    fn a_line_follows_up_only_within_eight_seconds_of_the_lamps_words() {
        let s = Duration::from_secs;
        assert!(follows_up(Some(Duration::ZERO)));
        assert!(follows_up(Some(s(5))));
        assert!(follows_up(Some(s(8))));
        assert!(!follows_up(Some(s(8) + Duration::from_millis(1))));
        assert!(!follows_up(Some(s(12))));
        assert!(!follows_up(None));
    }
}
