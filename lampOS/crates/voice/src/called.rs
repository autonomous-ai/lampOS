//! Telling the model it was called, while the person is still speaking
//! (`docs/attention.md`).
//!
//! The lamp hears its name in the room's sound (`docs/name.md`); the model
//! mostly does not, and so it sometimes takes a called question for other
//! people's talk. A second look (`crate::second_look`) mends that after
//! the fact, seconds late. This is the same sign given in time: when the
//! name is heard plainly as a request begins, a short written note goes to
//! the service inside that request, beside the sound, and the model has it
//! in hand when it decides whether to answer.
//!
//! The note is for the model alone and says so. It is sent once for a
//! request, only while that request's sound is still being sent, and never
//! on the strength of a faint hearing.

/// `on` for the note to be sent. Anything else: never.
pub const ENVIRONMENT: &str = "LAMP_NAME_NOTE";

/// What the model is given beside what it hears. It opens as the lines
/// about the room do, which the character file tells it nobody said.
pub const NOTE: &str = "(For you to know, not to answer: your own ears caught your name, Lamp, just now. If what is being said is something a lamp on a desk could answer or chat about, it is said to you. Other people's own business still is not yours.)";

/// The least a hearing scores to be passed on (`crate::second_look`).
pub const LEAST_SCORE: f32 = crate::second_look::LEAST_SCORE;
/// The name counts for a request if it began no longer than this before
/// the request's sound began to be sent.
pub const BEFORE_US: u64 = crate::attention::NAME_BEFORE_US;

/// The provider worker's flag for it: the worker is started with a clean
/// environment and is told everything by its arguments.
pub const FLAG: &str = "--name-note";

/// `face` for the note to wait, in a busy room, for someone turned to the
/// lamp. On lamp-4ace on 2026-10-14, in a meeting beside it, the part that
/// listens for the name heard it four times in twenty minutes in people's
/// talk (0.928 to 0.993), and after both notes it gave, the lamp answered
/// the table. A person who half hears their name in a meeting looks up; they
/// answer only someone looking at them. In a quiet room nothing changes.
pub const BUSY_ENVIRONMENT: &str = "LAMP_NAME_NOTE_BUSY";
/// The provider worker's flag for that, after [`FLAG`].
pub const BUSY_FLAG: &str = "--name-note-busy-face";

/// `on` for the note to say the name may have been heard as another word.
/// On lamp-4ace on 19 October Samantha's "Hey Lamp, tell me a story about a
/// brave little robot…" came back in ten takes as "Hey man" 4 times, "Hey
/// Mom" twice, "Hey ma'am" once and with no greeting 3 times; lamp-name heard
/// the name each time, and four "Hey Mom" requests in one arm got no answer
/// (#829, the first builder at 04:00 and 05:22). The model's own ears heard
/// someone else's name, and took the line as said to them.
pub const HEARD_AS_ENVIRONMENT: &str = "LAMP_NAME_NOTE_HEARD_AS";
/// The provider worker's flag for that, after [`FLAG`] and [`BUSY_FLAG`].
pub const HEARD_AS_FLAG: &str = "--name-note-heard-as";
/// [`NOTE`], saying too that the name may have sounded like another word.
pub const NOTE_HEARD_AS: &str = "(For you to know, not to answer: your own ears caught your name, Lamp, just now, even if it sounded to you like another word, such as \"Mom\", \"man\" or \"ma'am\". If what is being said is something a lamp on a desk could answer or chat about, it is said to you. Other people's own business still is not yours.)";

/// The provider worker's arguments for those switches in the environment.
pub fn arguments_from_environment() -> Vec<String> {
    let mut flags = arguments(
        std::env::var(ENVIRONMENT).ok().as_deref(),
        std::env::var(BUSY_ENVIRONMENT).ok().as_deref(),
    );
    flags.extend(heard_as_arguments(
        &flags,
        std::env::var(HEARD_AS_ENVIRONMENT).ok().as_deref(),
    ));
    flags
}

fn arguments(note: Option<&str>, busy: Option<&str>) -> Vec<String> {
    match (note, busy) {
        (Some("on"), Some("face")) => vec![FLAG.to_owned(), BUSY_FLAG.to_owned()],
        (Some("on"), _) => vec![FLAG.to_owned()],
        _ => Vec::new(),
    }
}

/// [`HEARD_AS_FLAG`] after the note's own flags, only when the note is sent.
fn heard_as_arguments(note_flags: &[String], heard_as: Option<&str>) -> Vec<String> {
    if !note_flags.is_empty() && heard_as == Some("on") {
        vec![HEARD_AS_FLAG.to_owned()]
    } else {
        Vec::new()
    }
}

/// Take a leading [`HEARD_AS_FLAG`] off a worker's flags, after [`take`].
pub fn take_heard_as(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == HEARD_AS_FLAG => (true, rest),
        _ => (false, flags),
    }
}

/// The note the model is given, with the name heard as another word or not.
pub fn note(heard_as: bool) -> &'static str {
    if heard_as { NOTE_HEARD_AS } else { NOTE }
}

/// Whether the environment switches it on, for the coordinator, which
/// then takes no second look at a request the model was told in.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// Take a leading `--name-note`, and `--name-note-busy-face` after it, off
/// a worker's flags: whether the note is sent, and whether in a busy room it
/// waits for someone turned to the lamp.
pub fn take(flags: &[String]) -> (bool, bool, &[String]) {
    match flags {
        [flag, busy, rest @ ..] if flag == FLAG && busy == BUSY_FLAG => (true, true, rest),
        [flag, rest @ ..] if flag == FLAG => (true, false, rest),
        _ => (false, false, flags),
    }
}

/// Whether a note due in a room this busy, with the eyes saying this of
/// who is turned to the lamp, is sent; and, when it is not, why.
pub fn busy_gate(needs_face: bool, busy: bool, turned: Option<bool>) -> Result<(), &'static str> {
    match (needs_face && busy, turned) {
        (false, _) | (true, Some(true)) => Ok(()),
        (true, Some(false)) => Err("nobody is turned to the lamp"),
        (true, None) => Err("the eyes have said nothing fresh"),
    }
}

#[derive(Debug, Default)]
pub struct Called {
    /// When the newest plain hearing of the name began.
    heard_us: Option<u64>,
    /// The request whose sound is being sent, and since when.
    open: Option<(u64, u64)>,
    /// The newest request a note was offered for.
    told: Option<u64>,
}

impl Called {
    /// The name began to be heard at `began_us`, with this score.
    pub fn name_heard(&mut self, began_us: u64, score: f32) {
        if score >= LEAST_SCORE {
            self.heard_us = Some(
                self.heard_us
                    .map_or(began_us, |before| before.max(began_us)),
            );
        }
    }

    /// `open`: the request whose sound is being sent now, if there is one.
    /// That request, if the model is to be told now that the lamp's name
    /// was heard: once for a request, and only for a hearing that began as
    /// the request did or since.
    pub fn due(&mut self, open: Option<u64>, now_us: u64) -> Option<u64> {
        let Some(request) = open else {
            self.open = None;
            return None;
        };
        let since = match self.open {
            Some((current, since)) if current == request => since,
            _ => {
                self.open = Some((request, now_us));
                now_us
            }
        };
        if self.told == Some(request) {
            return None;
        }
        let heard = self.heard_us?;
        (heard.saturating_add(BEFORE_US) >= since).then_some(request)
    }

    /// A note was offered for `request`, whether or not it could be sent.
    pub fn told(&mut self, request: u64) {
        self.told = Some(request);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #829: the heard-as line goes only with the note, after its flags,
    /// and only for `on`.
    #[test]
    fn the_heard_as_line_is_passed_only_with_the_note_and_only_for_on() {
        let note_flags = arguments(Some("on"), None);
        assert_eq!(heard_as_arguments(&note_flags, Some("on")), [HEARD_AS_FLAG]);
        for value in [None, Some(""), Some("off"), Some("ON")] {
            assert!(
                heard_as_arguments(&note_flags, value).is_empty(),
                "{value:?}"
            );
        }
        assert!(heard_as_arguments(&arguments(None, None), Some("on")).is_empty());
        let flags: Vec<String> = [FLAG, BUSY_FLAG, HEARD_AS_FLAG, "--next"]
            .map(str::to_owned)
            .into();
        let (on, busy, rest) = take(&flags);
        assert!(on && busy);
        let (heard_as, rest) = take_heard_as(rest);
        assert!(heard_as);
        assert_eq!(rest, ["--next"]);
        let (heard_as, rest) = take_heard_as(&flags[3..]);
        assert!(!heard_as);
        assert_eq!(rest, ["--next"]);
        assert_eq!(note(false), NOTE);
        assert_eq!(note(true), NOTE_HEARD_AS);
        assert!(NOTE_HEARD_AS.contains("\"Mom\""));
    }

    const SECOND: u64 = 1_000_000;

    #[test]
    fn a_request_that_begins_with_the_name_is_told_of_it_once() {
        let mut called = Called::default();
        // Nothing open: nothing to tell, whatever was heard.
        called.name_heard(10 * SECOND - 300_000, 1.0);
        assert_eq!(called.due(None, 10 * SECOND - 100_000), None);
        // The request opens a moment after the name began.
        assert_eq!(called.due(Some(4), 10 * SECOND), Some(4));
        called.told(4);
        for tick in 1..50 {
            assert_eq!(called.due(Some(4), 10 * SECOND + tick * 20_000), None);
        }
        // The next request has no name in it.
        assert_eq!(called.due(None, 14 * SECOND), None);
        assert_eq!(called.due(Some(5), 20 * SECOND), None);
        assert_eq!(called.due(Some(5), 22 * SECOND), None);
    }

    #[test]
    fn the_name_heard_in_the_middle_of_a_request_is_told_then() {
        let mut called = Called::default();
        assert_eq!(called.due(Some(1), 5 * SECOND), None);
        // "So I was thinking, Lamp, ...": the hearing comes a second in.
        called.name_heard(6 * SECOND, 0.995);
        assert_eq!(called.due(Some(1), 6 * SECOND + 400_000), Some(1));
    }

    #[test]
    fn a_faint_hearing_or_an_old_one_tells_nothing() {
        let mut called = Called::default();
        called.name_heard(10 * SECOND, 0.85);
        assert_eq!(called.due(Some(1), 10 * SECOND + 200_000), None);
        // Heard plainly, but long before this request began.
        let mut called = Called::default();
        called.name_heard(10 * SECOND, 1.0);
        assert_eq!(called.due(Some(2), 10 * SECOND + BEFORE_US + 1), None);
        // And it stays untold however long that request goes on.
        assert_eq!(called.due(Some(2), 30 * SECOND), None);
        // To the limit, it counts.
        let mut called = Called::default();
        called.name_heard(10 * SECOND, 1.0);
        assert_eq!(called.due(Some(3), 10 * SECOND + BEFORE_US), Some(3));
    }

    #[test]
    fn only_its_own_flag_at_the_front_turns_it_on() {
        let flags = |list: &[&str]| list.iter().map(|flag| flag.to_string()).collect::<Vec<_>>();
        let given = flags(&[FLAG, "--bus-root", "/run/lampos-bus"]);
        let (on, face, rest) = take(&given);
        assert!(on && !face);
        assert_eq!(rest, &given[1..]);
        let given = flags(&[FLAG, BUSY_FLAG, "--bus-root", "/run/lampos-bus"]);
        let (on, face, rest) = take(&given);
        assert!(on && face);
        assert_eq!(rest, &given[2..]);
        // Never the second without the first.
        let given = flags(&[BUSY_FLAG, "--bus-root", "/run/lampos-bus"]);
        let (on, face, rest) = take(&given);
        assert!(!on && !face);
        assert_eq!(rest, &given[..]);
        let given = flags(&["--bus-root", "/run/lampos-bus", FLAG]);
        let (on, _, rest) = take(&given);
        assert!(!on);
        assert_eq!(rest, &given[..]);
        assert!(!take(&[]).0);
        assert_eq!(arguments(Some("on"), Some("face")), [FLAG, BUSY_FLAG]);
        assert_eq!(arguments(Some("on"), Some("yes")), [FLAG]);
        assert!(arguments(None, Some("face")).is_empty());
    }

    /// In a busy room the note waits for someone turned to the lamp; in
    /// any other room, or with the switch off, it goes as before.
    #[test]
    fn in_a_busy_room_the_note_waits_for_someone_turned_to_the_lamp() {
        assert_eq!(busy_gate(true, true, Some(true)), Ok(()));
        assert_eq!(
            busy_gate(true, true, Some(false)),
            Err("nobody is turned to the lamp")
        );
        assert_eq!(
            busy_gate(true, true, None),
            Err("the eyes have said nothing fresh")
        );
        for turned in [Some(true), Some(false), None] {
            assert_eq!(busy_gate(true, false, turned), Ok(()));
            assert_eq!(busy_gate(false, true, turned), Ok(()));
        }
    }

    /// A note that could not be sent is not offered again and again.
    #[test]
    fn a_request_is_offered_its_note_once_whether_or_not_it_went() {
        let mut called = Called::default();
        called.name_heard(10 * SECOND, 1.0);
        assert_eq!(called.due(Some(7), 10 * SECOND + 100_000), Some(7));
        called.told(7);
        called.name_heard(11 * SECOND, 1.0);
        assert_eq!(called.due(Some(7), 11 * SECOND + 100_000), None);
    }
}
