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
//! The window is [`FOLLOW_UP_FOR`]'s 8 s unless `LAMP_FOLLOW_UP_FOR_MS`
//! sets it ([`FOR_ENVIRONMENT`]).
//!
//! Everything else is as without it: the name, a tap, the text calling the
//! lamp, said close to it, an answer to its own question inside the window,
//! its news, and the privacy and policy holds.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Where the coordinator reads the switch.
pub const ENVIRONMENT: &str = "LAMP_NAME_OR_FOLLOW_UP";
/// The provider worker's flag for it, right after `--live-scoped-session`,
/// read by both routes' workers.
pub const FLAG: &str = "--name-or-follow-up";

/// How long after the end of the lamp's own last words a line is still a
/// follow-up, let out as the conversation going on, unless
/// [`FOR_ENVIRONMENT`] sets it. Long enough for "And tomorrow?" after an
/// answer; short of the next lines of two people talking at a desk after it.
// ponytail: chosen, not measured; set it again from a real desk-talk set.
pub const FOLLOW_UP_FOR: Duration = Duration::from_secs(8);

/// Where the coordinator reads the window in place of [`FOLLOW_UP_FOR`], a
/// whole number of ms from [`FOR_LEAST`] to [`FOR_MOST`]: the lead's
/// decision 4 (#1711, 23 Oct 08:2x), a second person's follow-up without the
/// name answered within the window, so the window is a setting. Read only
/// with the switch on; anything else is said once and ignored.
pub const FOR_ENVIRONMENT: &str = "LAMP_FOLLOW_UP_FOR_MS";
/// The worker's flag for it, with its value, right after [`FLAG`].
pub const FOR_FLAG: &str = "--follow-up-for-ms";
/// The shortest window the setting takes: under it, "And tomorrow?" said
/// as the answer ends would need the name.
const FOR_LEAST: Duration = Duration::from_secs(1);
/// The longest: the second route's busy room keeps a conversation going on
/// only so long anyway
/// (`second_provider::worker::BUSY_GOING_ON_FOR`), so a longer window
/// would widen nothing there.
const FOR_MOST: Duration = Duration::from_secs(15);

/// The window the worker was given, in ms ([`FOLLOW_UP_FOR`] until then).
static FOR_MS: AtomicU64 = AtomicU64::new(FOLLOW_UP_FOR.as_millis() as u64);

/// The worker's flags for the switch, from the coordinator's environment.
pub fn arguments_from_environment() -> Vec<String> {
    let value = std::env::var(ENVIRONMENT).ok();
    let window = std::env::var(FOR_ENVIRONMENT).ok();
    if value.as_deref() == Some("on")
        && let Some(window) = window.as_deref()
        && within(window).is_none()
    {
        eprintln!(
            "lamp-voice: {FOR_ENVIRONMENT}={window:?} is not a whole number of ms from {} to {}; a follow-up has {} s",
            FOR_LEAST.as_millis(),
            FOR_MOST.as_millis(),
            seconds(FOLLOW_UP_FOR)
        );
    }
    arguments_with(value.as_deref(), window.as_deref())
}

fn arguments(value: Option<&str>) -> Vec<String> {
    if value == Some("on") {
        vec![FLAG.to_owned()]
    } else {
        Vec::new()
    }
}

/// [`arguments`], and the window's flag after the switch's when `window`
/// is one ([`within`]).
fn arguments_with(value: Option<&str>, window: Option<&str>) -> Vec<String> {
    let mut flags = arguments(value);
    if let Some(window) = window.and_then(within)
        && !flags.is_empty()
    {
        flags.extend([FOR_FLAG.to_owned(), window.as_millis().to_string()]);
    }
    flags
}

/// `value` as a window: a whole number of ms from [`FOR_LEAST`] to
/// [`FOR_MOST`].
fn within(value: &str) -> Option<Duration> {
    let window = Duration::from_millis(value.trim().parse().ok()?);
    (FOR_LEAST..=FOR_MOST).contains(&window).then_some(window)
}

/// Take a leading [`FLAG`] off a worker's flags.
pub fn take(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == FLAG => (true, rest),
        _ => (false, flags),
    }
}

/// Take a leading [`FOR_FLAG`] and its value off a worker's flags, right
/// after [`take`]: the window, when its value is one.
pub fn take_for(flags: &[String]) -> (Option<Duration>, &[String]) {
    match flags {
        [flag, value, rest @ ..] if flag == FOR_FLAG => (within(value), rest),
        [flag] if flag == FOR_FLAG => (None, &flags[1..]),
        _ => (None, flags),
    }
}

/// The worker's window from now on: what [`take_for`] took.
pub fn set_follow_up_for(window: Duration) {
    FOR_MS.store(window.as_millis() as u64, Ordering::Relaxed);
}

/// The window a follow-up has: [`FOLLOW_UP_FOR`], or the one set.
pub fn follow_up_for() -> Duration {
    Duration::from_millis(FOR_MS.load(Ordering::Relaxed))
}

/// `window` in seconds as the journal says it: "8", "7.5".
pub fn seconds(window: Duration) -> String {
    (window.as_millis() as f64 / 1000.0).to_string()
}

/// Whether a line said `ago` after the lamp's own last words ended (zero
/// while they still play; `None`: it said none) is a follow-up to them,
/// within [`follow_up_for`].
pub fn follows_up(ago: Option<Duration>) -> bool {
    follows_up_within(ago, follow_up_for())
}

/// [`follows_up`] with the window `window`.
fn follows_up_within(ago: Option<Duration>, window: Duration) -> bool {
    ago.is_some_and(|ago| ago <= window)
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

    /// `LAMP_FOLLOW_UP_FOR_MS` (the lead's decision 4, 23 Oct): a whole
    /// number of ms from 1000 to the busy room's 15 s goes to the worker
    /// after the switch's flag; anything else, or the switch off, gives none
    /// and the window stays 8 s.
    #[test]
    fn the_window_is_a_setting_from_one_to_fifteen_seconds_after_the_switch() {
        assert_eq!(
            arguments_with(Some("on"), Some("5000")),
            [FLAG, FOR_FLAG, "5000"]
        );
        assert_eq!(
            arguments_with(Some("on"), Some(" 1000 ")),
            [FLAG, FOR_FLAG, "1000"]
        );
        assert_eq!(
            arguments_with(Some("on"), Some("15000")),
            [FLAG, FOR_FLAG, "15000"]
        );
        for wrong in [
            None,
            Some(""),
            Some("999"),
            Some("15001"),
            Some("8 s"),
            Some("-1"),
            Some("8000.5"),
        ] {
            assert_eq!(arguments_with(Some("on"), wrong), [FLAG], "{wrong:?}");
        }
        assert!(arguments_with(None, Some("5000")).is_empty());
        assert!(arguments_with(Some("off"), Some("5000")).is_empty());
    }

    /// The worker takes the window's flag and value right after the switch's,
    /// and leaves the rest; a value it can't read, or none, is no window.
    #[test]
    fn the_worker_takes_the_window_after_the_switch() {
        let flags = |list: &[&str]| list.iter().map(|flag| flag.to_string()).collect::<Vec<_>>();
        let given = flags(&[FOR_FLAG, "5000", "x"]);
        assert_eq!(
            take_for(&given),
            (Some(Duration::from_secs(5)), &given[2..])
        );
        let none = flags(&["x"]);
        assert_eq!(take_for(&none), (None, &none[..]));
        for wrong in [
            flags(&[FOR_FLAG, "20000", "x"]),
            flags(&[FOR_FLAG, "soon", "x"]),
        ] {
            assert_eq!(take_for(&wrong), (None, &wrong[2..]), "{wrong:?}");
        }
        let alone = flags(&[FOR_FLAG]);
        assert_eq!(take_for(&alone), (None, &alone[1..]));
    }

    /// With a 5 s window a line 5 s after is a follow-up and one 6 s after
    /// isn't; with 12 s, one 12 s after is. Unset, it is 8 s.
    #[test]
    fn a_line_follows_up_within_the_window_set() {
        let s = Duration::from_secs;
        assert!(follows_up_within(Some(s(5)), s(5)));
        assert!(!follows_up_within(Some(s(6)), s(5)));
        assert!(follows_up_within(Some(s(12)), s(12)));
        assert!(follows_up_within(Some(Duration::ZERO), s(1)));
        assert!(!follows_up_within(None, s(15)));
        assert_eq!(follow_up_for(), FOLLOW_UP_FOR);
        assert_eq!(seconds(FOLLOW_UP_FOR), "8");
        assert_eq!(seconds(Duration::from_millis(7500)), "7.5");
    }
}
