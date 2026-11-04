//! A reply that says a light, volume or Schedule action was done, with no
//! call of that kind in its turn, is never said as it is
//! (`LAMP_ACTION_CLAIM_CHECKED`, #5215).
//!
//! On lamp-52e6 at build 9928 (4 Nov 09:38) "Lamp, set a timer to one
//! minute." was heard right, no chat round called anything, and the lamp
//! said "Timer set. It rings at 9:39 am.": no timer existed. In an earlier
//! run 4 of 14 light and volume asks got "Louder." (twice), "Brighter." or
//! "Cooler." and no call, so nothing changed. The model copies the acting
//! part's own lines, kept in the thread as the lamp's words.
//!
//! The test is strict, never a phrase list: the reply, its case, spaces and
//! curly apostrophes aside, is or opens with one of the acting part's own
//! lines, followed by its end or a sentence mark ('.', '!', ',', ';', ':',
//! a dash), and never a '?':
//! - the executive's lines after a light or volume call ([`SAID_LINES`],
//!   `lamp_actions::SAID_LINES`). Not its line for a colour made ("My light
//!   is blue now."): with `LAMP_LIGHT_TOLD` it is also how a turn answers
//!   "what colour is the light?", and kept colour calls carry their colour
//!   since #5101 (the first builder's review of #5215);
//! - the Schedule's set lines as the second route says them
//!   (`second_provider::timer_line`): "Timer set." and "I'll remind you at
//!   H:MM am." (or pm). The second is also an answer to "when will you
//!   remind me?", so a `list_reminders` call in the turn makes it stand
//!   ([`Claim::ReminderTime`]); "Timer set." needs a set.
//!
//! Such a claim stands when its turn made a call of its kind
//! ([`Claim::made_by`]); otherwise it is held, the model is asked once more
//! with [`NOTE`], and a second claim with no call is said as
//! [`NOT_DONE_LINE`]. Anything else, an answer to a question included, is
//! untouched. Off unless the environment says exactly `on`.

/// Where the second worker reads the switch. `on` only.
pub const ENVIRONMENT: &str = "LAMP_ACTION_CLAIM_CHECKED";

/// Whether the switch is on in this process's environment.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// The executive's own lines after a light or volume call
/// (`lamp_actions::short_say` and `already_say`): a test holds this equal
/// to `lamp_actions::SAID_LINES` (the voice links lamp-actions only in its
/// tests).
pub const SAID_LINES: [&str; 14] = [
    "Brighter.",
    "Dimmer.",
    "Warmer.",
    "Cooler.",
    "Louder.",
    "Quieter.",
    "Done.",
    "I'm already as bright as I go.",
    "I'm already as dim as I go.",
    "I'm already as warm as I go.",
    "I'm already as cool as I go.",
    "I'm already as loud as I go.",
    "I'm already as quiet as I go.",
    "It already is.",
];

/// Of [`SAID_LINES`], the two any call may end in ("Done." for calls that
/// differ, `second_provider::SHORT_DONE_TOGETHER`): any call of the turn
/// answers them.
const ANY_CALL_LINES: [&str; 2] = ["Done.", "It already is."];

/// The Schedule's set line for a timer, as `second_provider::timer_line`
/// says it, to its first sentence's end.
const TIMER_SET: &str = "Timer set.";
/// The Schedule's set line for a timer asked as a reminder
/// (`second_provider::timer_line`), up to its clock.
const REMIND_AT: &str = "I'll remind you at ";

/// What follows a claim's words for it to be one: its end, or a sentence
/// mark. Never a '?': "Brighter?" asks.
const MARKS: [char; 7] = ['.', '!', ',', ';', ':', '\u{2014}', '\u{2013}'];
/// Dashes that mark a claim's end after one space too ("Done \u{2014} it's red.").
const DASHES: [char; 3] = ['\u{2014}', '\u{2013}', '-'];

/// Told before the round asked again after a claim with no call.
pub const NOTE: &str = "(For you to know: your last reply said something was done, but no function was called in this turn, so nothing was done, and it was not said. If they asked for a change, a timer or a reminder, call its function now; the lamp says the result itself. If you cannot do it, say plainly that you didn't. If they only asked a question, answer it in words without saying that anything was done.)";

/// What the next round is told when a claim was held beside calls of
/// another kind, which did run (Thirteen's P2 on #5253): never "no function
/// was called", so the model doesn't call those again (a second timer, a
/// light made brighter twice), and only the claimed kind is asked for.
pub fn beside_calls_note(kind: Claim) -> String {
    format!(
        "(For you to know: your last reply said {} was done, but no function for it was called, so it was not done, and those words were not said. The other functions in that reply did run: do not call them again. If they asked for {}, call its function now; the lamp says the result itself. If not, or if you cannot, say plainly that it wasn't done.)",
        kind.what(),
        kind.what()
    )
}

/// Said when the round asked again claims again with no call.
pub const NOT_DONE_LINE: &str = "Sorry, I didn't do that. Could you ask me again?";

/// The kind of action a reply claims.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Claim {
    /// A light or volume line ("Louder.").
    LightOrVolume,
    /// "Done." or "It already is.": any call's line.
    AnyCall,
    /// A Schedule's set line ("Timer set.").
    Schedule,
    /// "I'll remind you at H:MM am.": a set line, or what a list answers.
    ReminderTime,
}

impl Claim {
    /// Whether a call of `name` makes this claim true.
    pub fn made_by(self, name: &str) -> bool {
        match self {
            Self::LightOrVolume => crate::tools::is_light_or_volume(name),
            Self::AnyCall => true,
            Self::Schedule => matches!(
                name,
                "set_timer" | "set_reminder" | "repeat_reminder" | "snooze_reminder"
            ),
            Self::ReminderTime => Self::Schedule.made_by(name) || name == "list_reminders",
        }
    }

    /// What it claims, in the words a note tells the model.
    pub fn what(self) -> &'static str {
        match self {
            Self::LightOrVolume => "a change to the light or the volume",
            Self::AnyCall => "an action",
            Self::Schedule => "a timer or a reminder",
            Self::ReminderTime => "a reminder",
        }
    }

    /// Its name in the journal.
    pub fn label(self) -> &'static str {
        match self {
            Self::LightOrVolume => "light or volume",
            Self::AnyCall => "done",
            Self::Schedule => "timer or reminder",
            Self::ReminderTime => "reminder time",
        }
    }
}

/// `text` as the claim test reads it: lower case, straight apostrophes,
/// each run of spaces one, none before.
fn normalised(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.trim_start().chars() {
        if c.is_whitespace() {
            space = true;
            continue;
        }
        if space && !out.is_empty() {
            out.push(' ');
        }
        space = false;
        out.extend(if c == '\u{2019}' {
            '\''.to_lowercase()
        } else {
            c.to_lowercase()
        });
    }
    if space && !out.is_empty() {
        out.push(' ');
    }
    out
}

/// A line's words as [`normalised`] reads them, its closing '.' or '!' off.
fn stem(line: &str) -> String {
    normalised(line).trim_end_matches(['.', '!']).to_owned()
}

/// Whether `text` opens with `stem` and then its end or a mark: `None` while
/// more text could still decide it.
fn opens_with(text: &str, stem: &str, ended: bool) -> Option<bool> {
    match text.strip_prefix(stem) {
        Some(rest) => marked(rest, ended),
        None if !ended && stem.starts_with(text) => None,
        None => Some(false),
    }
}

/// Whether what follows a claim's words ends them: nothing more, a mark, or
/// one space and a dash. `None` while undecided.
fn marked(rest: &str, ended: bool) -> Option<bool> {
    let mut chars = rest.chars();
    match chars.next() {
        None if ended => Some(true),
        None => None,
        Some(' ') => match chars.next() {
            None if ended => Some(true),
            None => None,
            Some(c) => Some(DASHES.contains(&c)),
        },
        Some(c) => Some(MARKS.contains(&c)),
    }
}

/// Whether `rest` opens with a clock as the lamp says one ("9:39 am",
/// "12:05 pm") and then its end or a mark: `None` while undecided.
fn opens_with_a_clock(rest: &str, ended: bool) -> Option<bool> {
    let mut undecided = false;
    for shape in ["0:00 xm", "00:00 xm"] {
        let mut chars = rest.chars();
        let mut fits = true;
        let mut short = false;
        for want in shape.chars() {
            let Some(c) = chars.next() else {
                short = true;
                break;
            };
            let ok = match want {
                '0' => c.is_ascii_digit(),
                'x' => c == 'a' || c == 'p',
                want => c == want,
            };
            if !ok {
                fits = false;
                break;
            }
        }
        if !fits {
            continue;
        }
        if short {
            undecided |= !ended;
            continue;
        }
        match marked(chars.as_str(), ended) {
            Some(true) => return Some(true),
            None => undecided = true,
            Some(false) => {}
        }
    }
    if undecided { None } else { Some(false) }
}

/// Whether `reply` claims an action as the acting part's own lines say
/// one (the module's rule): `Some(Some(kind))`, a claim; `Some(None)`, not
/// one; `None`, not yet known from the text so far. `ended`: no more text
/// comes.
pub fn claim(reply: &str, ended: bool) -> Option<Option<Claim>> {
    let mut text = normalised(reply);
    if ended {
        text.truncate(text.trim_end().len());
    }
    if text.is_empty() {
        return if ended { Some(None) } else { None };
    }
    let mut undecided = false;
    let mut judge = |found: Option<bool>, kind: Claim| -> Option<Claim> {
        match found {
            Some(true) => Some(kind),
            None => {
                undecided = true;
                None
            }
            Some(false) => None,
        }
    };
    let lines = SAID_LINES
        .iter()
        .map(|line| {
            let kind = if ANY_CALL_LINES.contains(line) {
                Claim::AnyCall
            } else {
                Claim::LightOrVolume
            };
            (stem(line), kind)
        })
        .chain(std::iter::once((stem(TIMER_SET), Claim::Schedule)));
    for (stem, kind) in lines {
        if let Some(kind) = judge(opens_with(&text, &stem, ended), kind) {
            return Some(Some(kind));
        }
    }
    let remind = normalised(REMIND_AT);
    let found = match text.strip_prefix(remind.as_str()) {
        Some(rest) => opens_with_a_clock(rest, ended),
        None if !ended && remind.starts_with(text.as_str()) => None,
        None => Some(false),
    };
    if let Some(kind) = judge(found, Claim::ReminderTime) {
        return Some(Some(kind));
    }
    if undecided { None } else { Some(None) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #5215: the lines are the executive's own, each once, so a line it
    /// says after a real call and one the model copies are the same words.
    #[test]
    fn the_said_lines_are_the_executives_own() {
        assert_eq!(SAID_LINES, lamp_actions::SAID_LINES);
        for line in ANY_CALL_LINES {
            assert!(SAID_LINES.contains(&line), "{line}");
        }
        assert_eq!(
            crate::second_provider::SHORT_DONE_TOGETHER,
            ANY_CALL_LINES[0]
        );
    }

    /// The Schedule's set lines are the second route's own
    /// (`second_provider::timer_line`).
    #[test]
    fn the_schedule_lines_are_the_timer_lines_own() {
        use crate::second_provider::net::ToolCall;
        let calls = [ToolCall {
            id: "c0".into(),
            name: "set_timer".into(),
            args: serde_json::json!({}),
            extra: None,
        }];
        let told = [serde_json::json!({"status": "committed", "clock": "09:39"})];
        for reminded in [false, true] {
            let line = crate::second_provider::timer_line(&calls, &told, reminded).unwrap();
            let kind = if reminded {
                Claim::ReminderTime
            } else {
                Claim::Schedule
            };
            assert_eq!(claim(&line, true), Some(Some(kind)), "{line}");
            assert!(
                line.starts_with(TIMER_SET) || line.starts_with(REMIND_AT),
                "{line}"
            );
        }
    }

    #[test]
    fn a_reply_that_is_or_opens_with_an_own_line_is_a_claim() {
        for (reply, kind) in [
            ("Louder.", Claim::LightOrVolume),
            ("louder", Claim::LightOrVolume),
            ("  Brighter! ", Claim::LightOrVolume),
            ("Brighter. Better?", Claim::LightOrVolume),
            ("Cooler, as you like.", Claim::LightOrVolume),
            ("I\u{2019}m already as loud as I go.", Claim::LightOrVolume),
            ("Done.", Claim::AnyCall),
            ("Done \u{2014} it's red.", Claim::AnyCall),
            ("Louder - there.", Claim::LightOrVolume),
            ("It already is.", Claim::AnyCall),
            ("Timer set. It rings at 9:39 am.", Claim::Schedule),
            ("Timer set.", Claim::Schedule),
            ("I'll remind you at 12:03 pm.", Claim::ReminderTime),
            ("I'll remind you at 9:39 am", Claim::ReminderTime),
        ] {
            assert_eq!(claim(reply, true), Some(Some(kind)), "{reply:?}");
        }
    }

    #[test]
    fn anything_else_is_no_claim() {
        for reply in [
            "It's 9:39.",
            "What time is it?",
            "You have one timer, for tea, at 9:39 am.",
            "Louder music helps me focus.",
            "Brighter?",
            "Done?",
            "Doneness matters for steak.",
            "Timer settings are in the app.",
            "My light is blue.",
            "My light is blue now.",
            "My light is warm white now.",
            "My light is blue now?",
            "I'll remind you at nine.",
            "I'll remind you at 9:39 tomorrow.",
            "It rings at 9:39 am.",
            "Okay, louder.",
            "",
        ] {
            assert_eq!(claim(reply, true), Some(None), "{reply:?}");
        }
    }

    /// Streamed, a claim is undecided until its words and the mark after
    /// them are in, and decided no claim at the first that fits none.
    #[test]
    fn a_reply_so_far_is_judged_as_it_comes() {
        for reply in [
            "",
            "Lou",
            "Louder",
            "Timer",
            "Timer set",
            "I'll remind you at 9:3",
        ] {
            assert_eq!(claim(reply, false), None, "{reply:?}");
        }
        assert_eq!(claim("Louder.", false), Some(Some(Claim::LightOrVolume)));
        assert_eq!(claim("Timer set.", false), Some(Some(Claim::Schedule)));
        assert_eq!(
            claim("I'll remind you at 9:39 am.", false),
            Some(Some(Claim::ReminderTime))
        );
        for reply in ["Lu", "Louder m", "The", "It's", "I'll remind you at n"] {
            assert_eq!(claim(reply, false), Some(None), "{reply:?}");
        }
    }

    #[test]
    fn a_claim_stands_beside_a_call_of_its_kind() {
        assert!(Claim::LightOrVolume.made_by("light_brighter"));
        assert!(Claim::LightOrVolume.made_by("volume_up"));
        assert!(!Claim::LightOrVolume.made_by("set_timer"));
        assert!(!Claim::LightOrVolume.made_by("volume_to"));
        assert!(Claim::Schedule.made_by("set_timer"));
        assert!(Claim::Schedule.made_by("repeat_reminder"));
        assert!(!Claim::Schedule.made_by("list_reminders"));
        assert!(Claim::ReminderTime.made_by("list_reminders"));
        assert!(Claim::ReminderTime.made_by("set_reminder"));
        assert!(!Claim::ReminderTime.made_by("light_brighter"));
        assert!(!Claim::Schedule.made_by("light_brighter"));
        assert!(Claim::AnyCall.made_by("set_timer"));
    }
}
