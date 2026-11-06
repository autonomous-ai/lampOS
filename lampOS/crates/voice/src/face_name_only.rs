//! The face name is the only name the lamp calls a person (#5484, U1 of the
//! lamp-29e7 report on #5385, `LAMP_SECOND_FACE_NAME_ONLY`).
//!
//! On lamp-29e7 (build 10226, 4 Nov) a known face saved as B was called A,
//! then B, then told "I don't know anyone called A": the conversation's
//! running summary said "The person's name is A", the face note said B, and
//! the model answered from whichever it read. With this switch on, on the
//! second route:
//!
//! - a turn of the known face's own person is told their face name as the
//!   only name to call them ([`note`]), in place of
//!   [`crate::known_faces::note`];
//! - every line of the running summary that gives a person's name
//!   ([`names_the_person`]) is told, and read by the observation pass, as a
//!   name said in the conversation, never what to call them
//!   ([`relabelled`]), with a face or without one.
//!
//! The owner's typed name from the app, when the reply is told it, is named
//! beside the face name as theirs too (Thirteen's P2 on #5516): only a
//! summary's or memory's name is one said in the conversation. A "call me
//! Ash" is used while its line is in view and marked once the summary folds
//! it: meant, since a known face's rename is the confirm of #5485.
//!
//! The summary is never rewritten: only how it is told. Off, both read
//! byte for byte as before.

use std::sync::atomic::{AtomicBool, Ordering};

/// `on`, and nothing else; read once as the second worker starts.
pub const ENVIRONMENT: &str = "LAMP_SECOND_FACE_NAME_ONLY";

/// What marks a running summary's line that names a person.
pub const SAID: &str = "(a name said in the conversation, never what to call them) ";

static ON: AtomicBool = AtomicBool::new(false);

/// Whether `value` turns the switch on: the exact `on`.
pub fn on_from(value: Option<&str>) -> bool {
    value == Some("on")
}

/// Set once by the second worker from [`ENVIRONMENT`].
pub fn set(on: bool) {
    ON.store(on, Ordering::Release);
}

/// Whether the switch is on in this process.
pub fn on() -> bool {
    ON.load(Ordering::Acquire)
}

/// What a turn of the known face's own person is told: their face name, the
/// name to call them, and the owner's `typed` name from the app beside it
/// when this reply is told that too and it is not the same name.
pub fn note(name: &str, typed: Option<&str>) -> String {
    const SAID_ONLY: &str = "A name in your running note, your memory or what was said earlier is only a name said in the conversation, never what to call them.";
    match typed
        .map(str::trim)
        .filter(|typed| !typed.is_empty() && !typed.eq_ignore_ascii_case(name.trim()))
    {
        Some(typed) => format!(
            "(This person is {name}: you know their face. Their name in the app is {typed}: both are their names, so call them {name} or {typed}. {SAID_ONLY})"
        ),
        None => format!(
            "(This person is {name}: you know their face. Call them {name}, the only name you call them. {SAID_ONLY})"
        ),
    }
}

/// Words that stand for the person a summary is about.
const PERSON_WORDS: [&str; 15] = [
    "user", "person", "owner", "their", "they", "my", "his", "her", "he", "she", "you", "your",
    "i", "guest", "visitor",
];

/// Whether a summary's `line` gives the person's own name: "The user's name
/// is A", "Their name is A", "My name is A", "The person is called A",
/// "He goes by A", "A's nickname is B", "I'm A", "Call me A". Not a pet's
/// or a thing's ("Their dog is called Biscuit", "Their dog's name is
/// Biscuit"), nor "I am tired".
pub fn names_the_person(line: &str) -> bool {
    let line = line.replace('\u{2019}', "'");
    let words: Vec<&str> = line
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|word| !word.is_empty())
        .collect();
    let lower: Vec<String> = words.iter().map(|word| word.to_lowercase()).collect();
    let person = |at: usize| {
        let word = lower[at].trim_end_matches("'s");
        PERSON_WORDS.contains(&word)
            || (lower[at].ends_with("'s") && words[at].starts_with(char::is_uppercase))
    };
    let capital = |at: usize| {
        words
            .get(at)
            .is_some_and(|word| word.starts_with(char::is_uppercase))
    };
    (0..lower.len()).any(|at| match lower[at].as_str() {
        "name" | "names" | "nickname" => at > 0 && person(at - 1),
        "called" | "named" => {
            at > 1 && matches!(lower[at - 1].as_str(), "is" | "are" | "was") && person(at - 2)
        }
        "by" => at > 1 && lower[at - 1] == "goes" && person(at - 2),
        "i'm" => capital(at + 1),
        "am" => at > 0 && lower[at - 1] == "i" && capital(at + 1),
        "me" => at > 0 && lower[at - 1] == "call",
        _ => false,
    })
}

/// `summary` with each line giving the person's name marked [`SAID`],
/// after any "- " that begins it; other lines as they are.
pub fn relabelled(summary: &str) -> String {
    summary
        .split_inclusive('\n')
        .map(|line| {
            if !names_the_person(line) || line.contains(SAID) {
                return line.to_owned();
            }
            let body = line.trim_start();
            let indent = &line[..line.len() - body.len()];
            match body.strip_prefix("- ") {
                Some(rest) => format!("{indent}- {SAID}{rest}"),
                None => format!("{indent}{SAID}{body}"),
            }
        })
        .collect()
}

/// `summary` without its lines giving the person's name.
fn without_names(summary: &str) -> String {
    summary
        .split_inclusive('\n')
        .filter(|line| !names_the_person(line))
        .collect()
}

/// [`relabelled`] when `on`, else `summary` as it is.
pub fn told(summary: String, on: bool) -> String {
    if on && !summary.is_empty() {
        relabelled(&summary)
    } else {
        summary
    }
}

/// [`told`] for a summary already fitted to `room` bytes: marked when the
/// marks fit too, else its name lines left out, so its other facts are
/// kept and it never grows past `room`.
pub fn told_within(summary: String, on: bool, room: usize) -> String {
    if !on || summary.is_empty() {
        return summary;
    }
    let marked = relabelled(&summary);
    if marked.len() < room {
        marked
    } else {
        without_names(&summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// lamp-29e7's summary: the name line is marked, the rest untouched,
    /// and marking twice marks once.
    #[test]
    fn a_summarys_name_line_is_told_as_a_name_said_in_the_conversation() {
        let summary = "Current activity: none\nKey facts:\n- The user's name is Ash.\n- Likes tea, no sugar.\n";
        let told = relabelled(summary);
        assert_eq!(
            told,
            format!(
                "Current activity: none\nKey facts:\n- {SAID}The user's name is Ash.\n- Likes tea, no sugar.\n"
            )
        );
        assert_eq!(relabelled(&told), told, "once");
        assert_eq!(told_off(summary), summary);
        assert_eq!(super::told(String::new(), true), "");
        // Past the room, the name line is left out and the facts kept.
        let tight = told_within(summary.to_owned(), true, summary.len() + 1);
        assert_eq!(
            tight,
            "Current activity: none\nKey facts:\n- Likes tea, no sugar.\n"
        );
        assert_eq!(told_within(summary.to_owned(), true, 4096), told);
        assert_eq!(told_within(summary.to_owned(), false, 1), summary);
    }

    /// Thirteen's P3 on #5516: only the person's own name is marked.
    #[test]
    fn only_a_line_giving_the_persons_name_is_marked() {
        for named in [
            "- The user's name is Ash.",
            "- The person's name is Ash.",
            "Their name is Minh",
            "My name is Dylan",
            "The user is called Ash",
            "He goes by Sam",
            "Dylan's nickname is Dyl",
            "I'm Dylan",
            "I\u{2019}m Dylan",
            "I am Dylan",
            "Call me Dyl",
        ] {
            assert!(names_the_person(named), "{named}");
        }
        for not in [
            "- Their dog is called Biscuit.",
            "- Their dog's name is Biscuit.",
            "- The game is named Twenty Questions.",
            "- Takes tea, no sugar.",
            "I am tired",
            "rename the file",
        ] {
            assert!(!names_the_person(not), "{not}");
        }
    }

    /// Thirteen's P2 on #5516: with the owner's typed name told too, the
    /// note names both as theirs; the same name, or none, the one.
    #[test]
    fn the_owners_typed_name_is_named_beside_the_face_name() {
        let both = note("Bryony", Some("Bree"));
        assert!(both.contains("This person is Bryony"), "{both}");
        assert!(both.contains("Their name in the app is Bree"), "{both}");
        assert!(!both.contains("the only name"), "{both}");
        for typed in [None, Some("bryony"), Some(" ")] {
            let one = note("Bryony", typed);
            assert!(one.contains("Call them Bryony, the only name"), "{one}");
        }
    }

    fn told_off(summary: &str) -> String {
        super::told(summary.to_owned(), false)
    }

    #[test]
    fn the_switch_is_the_exact_on() {
        assert!(on_from(Some("on")));
        for value in [None, Some("1"), Some("ON"), Some("on ")] {
            assert!(!on_from(value), "{value:?}");
        }
    }
}
