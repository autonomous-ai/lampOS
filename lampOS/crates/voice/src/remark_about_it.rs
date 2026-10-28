//! A remark about the lamp is not a call, on the second route in a busy room
//! (#4175), off unless `LAMP_REMARK_ABOUT_IT=on`.
//!
//! On lamp-4ace on 28 October, in build 8327's busy_day check, two
//! colleagues talked at a desk. "That lamp is cute, where did you get it?"
//! was heard as "That lamp is cute." and let out for the lamp's name heard
//! in the room's sound; the lamp answered "Thanks. I'd blush if I could."
//! Its own answer then made a conversation going on, and the next two desk
//! lines ("Do you want coffee? I'm going downstairs.", heard as "Do you want
//! confidence?", and "Yes please, a latte, no sugar.") were answered as part
//! of it.
//!
//! With the switch, in a busy room:
//! - a line let out only for the name heard ([`talked_about`]: every "lamp"
//!   in its words follows "that", "this", "the", "your" and the like), with
//!   no face turned to the lamp, no tap and no call in its words, is held:
//!   the lamp was talked about, not to, and a living thing says nothing;
//! - a line let out for the name heard in the room's sound alone, with no
//!   call in its words, no face turned and no tap, opens no conversation:
//!   its answer neither starts the call's window nor renews the one going
//!   on, so the next line needs a sign of its own.
//!
//! A line said to the lamp ("Lamp, is it cute?", "Hey lamp") keeps its call:
//! its "lamp" follows no determiner. So does one that asks or tells it
//! something with the lamp as its object ("Turn off the lamp.", "Can you dim
//! the lamp?", "Is the lamp on?"): a clause that names the lamp and asks
//! anything is no remark ([`crate::remarks::asks_in`]). Without the switch,
//! as before.

/// Where the second route reads the switch.
pub const ENVIRONMENT: &str = "LAMP_REMARK_ABOUT_IT";

/// Why the rule holds a remark, as its journal says it after "held back: ".
pub const HELD: &str =
    "a busy room: the lamp was talked about, not to (a remark about it), so it says nothing";

/// Whether the switch is on.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).is_ok_and(|value| value == "on")
}

/// Words that make the "lamp" after them a thing talked about.
const ABOUT: &[&str] = &[
    "that", "this", "the", "your", "my", "a", "our", "his", "her", "their", "these", "those",
];

/// Whether `heard` names the lamp only as a thing talked about: it says
/// "lamp" at least once, every "lamp" follows one of [`ABOUT`], and no
/// sentence or clause that names it asks or tells anything
/// ([`crate::remarks::asks_in`]): "Turn off the lamp." and "Can you dim the
/// lamp?" are said to it (Fourth's and the Reviewer-three's P2 on #4201). A
/// line with no "lamp" in its words is not judged here: its call may have
/// been lost in the hearing.
pub fn talked_about(heard: &str) -> bool {
    let asked = heard
        .split_inclusive(['.', '!', '?', ';', ','])
        .filter(|clause| named(clause))
        .any(|clause| crate::remarks::asks_in(clause) || moves_or_needs(clause));
    !asked && only_about(heard)
}

/// Words that ask the lamp to move when a clause begins with them ("Tilt
/// the lamp up.", "Point the lamp at my desk."): its head's own moves, which
/// [`crate::remarks::asks_in`]'s doing words leave out (Reviewer-three's
/// narrower P2 on #4201).
const MOVES: &[&str] = &[
    "lower", "raise", "lift", "tilt", "point", "aim", "move", "face", "nod", "lean", "angle",
    "bend", "swing", "rotate", "bring", "shine", "light",
];

/// Words that state a need of the lamp's light or place anywhere in a
/// clause ("The lamp is too bright.", "I need the lamp brighter."): a
/// missed request costs more in a busy room than a reply to a remark.
const NEEDS: &[&str] = &[
    "too", "need", "needs", "want", "wants", "brighter", "dimmer", "darker", "lighter", "warmer",
    "cooler", "higher",
];

/// Whether a clause naming the lamp asks it to move, or states a need of
/// its light or place.
fn moves_or_needs(clause: &str) -> bool {
    let mut words = clause
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .skip_while(|word| {
            matches!(
                word.as_str(),
                "and" | "so" | "but" | "then" | "okay" | "ok" | "now" | "um"
            )
        })
        .peekable();
    if words
        .peek()
        .is_some_and(|first| MOVES.contains(&first.as_str()))
    {
        return true;
    }
    words.any(|word| NEEDS.contains(&word.as_str()))
}

/// Whether a clause says "lamp" or "lamb" (one or many, or its own).
fn named(clause: &str) -> bool {
    clause
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .map(str::to_lowercase)
        .any(|word| {
            matches!(
                word.trim_end_matches("'s"),
                "lamp" | "lamps" | "lamb" | "lambs"
            )
        })
}

/// Whether `heard` says "lamp" (or "lamb") and every one follows one of
/// [`ABOUT`].
fn only_about(heard: &str) -> bool {
    let words: Vec<String> = heard
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect();
    let mut said = false;
    for (index, word) in words.iter().enumerate() {
        let word = word.trim_end_matches("'s");
        // "Lamb" is the name's own mishearing (`called_by_real_name`; the
        // Tests lane's P3 on #4201).
        if !matches!(word, "lamp" | "lamps" | "lamb" | "lambs") {
            continue;
        }
        said = true;
        let before = index.checked_sub(1).map(|before| words[before].as_str());
        if !before.is_some_and(|before| ABOUT.contains(&before)) {
            return false;
        }
    }
    said
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_remark_about_the_lamp_is_talked_about() {
        for heard in [
            "That lamp is cute.",
            "That lamp is cute, where did you get it?",
            "I love your lamp's light.",
            "That lamp is so cute, where did you get it?",
            "That lamp is cute. Where did you get it?",
            "That lamb is cute.",
        ] {
            assert!(talked_about(heard), "{heard}");
        }
    }

    #[test]
    fn a_line_said_to_the_lamp_is_not() {
        for heard in [
            "Lamp, is it cute?",
            "Hey lamp, what time is it?",
            "Lamp what's the weather",
            "Thanks lamp.",
            "That lamp is cute. Lamp, are you listening?",
            // Said to it, the lamp as the thing to act on (#4201's P2).
            "Turn off the lamp.",
            "Can you dim the lamp?",
            "Can you dim the lamp a little?",
            "Mainland. Turn off the lamp please.",
            "Make the lamp brighter, please.",
            "Make the lamp brighter.",
            "Dim the lamp, please.",
            "Shut off the lamp.",
            "Shut the lamp off.",
            "Is the lamp on?",
            // Its moves and a stated need (Reviewer-three's narrower P2 on
            // #4201).
            "Lower the lamp.",
            "Raise the lamp a bit.",
            "Point the lamp at my desk.",
            "Tilt the lamp up.",
            "The lamp is too bright.",
            "I need the lamp brighter.",
            "Do you want confidence? Going downstairs.",
            "Yes, please. A latte. No sugar.",
            "",
        ] {
            assert!(!talked_about(heard), "{heard}");
        }
    }
}
