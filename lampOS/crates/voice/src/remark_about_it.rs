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

/// Why the rule holds the line said right after a remark it held, let out
/// only as the conversation going on, not in the conversation's own voice
/// and answering no question of the lamp's: it carries on the talk about
/// the lamp to someone else. On lamp-4ace on 29 Oct at 20:3x
/// (`rUTNR-2031`), "It's from the office, they're testing it." after "That
/// lamp is cute." was answered as a conversation with the lamp going on.
pub const AFTER_A_REMARK: &str =
    "said right after a remark about the lamp, to someone else, with no sign of its own";

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
    // "Please" asks across its whole sentence, set off by a comma or not
    // ("The lamp, off please."; Reviewer-three's P3 on #4219). "You" and a
    // `?` stay with their clause: "That lamp is cute, where did you get it?"
    // asks the colleague, not the lamp.
    let pleased = heard
        .split_inclusive(['.', '!', '?', ';'])
        .filter(|sentence| named(sentence))
        .any(|sentence| {
            sentence
                .split(|c: char| !c.is_alphanumeric())
                .any(|word| word.eq_ignore_ascii_case("please"))
        });
    !asked && !pleased && only_about(heard)
}

/// Words that ask the lamp to move when a clause begins with them ("Tilt
/// the lamp up.", "Point the lamp at my desk."): its head's own moves, which
/// [`crate::remarks::asks_in`]'s doing words leave out (Reviewer-three's
/// narrower P2 on #4201).
const MOVES: &[&str] = &[
    "lower", "raise", "lift", "tilt", "point", "aim", "move", "face", "nod", "lean", "angle",
    "bend", "swing", "rotate", "bring", "shine", "light", "wake",
];

/// Light or place words that state a need after "too", "so" or "really"
/// ("The lamp is too bright.", "The lamp is so bright."; Reviewer-three's
/// P3 on #4239). Bare "too" is no need: "That lamp is cute too.", "That
/// lamp is way too cute." (Thirteen's P2 on #4219).
const TOO_WHAT: &[&str] = &[
    "bright", "dim", "dark", "high", "low", "close", "far", "warm", "cold", "harsh", "hot",
    "yellow", "blue", "white", "strong", "weak",
];

/// Words before a [`TOO_WHAT`] word that make it a complaint.
const TOO: &[&str] = &["too", "so", "really"];

/// Words after the lamp that say its light hurts ("This lamp hurts my
/// eyes.", "The lamp is blinding me."; Reviewer-three's P3 on #4239).
const HURTS: &[&str] = &["hurts", "blinding", "blinds"];

/// Comparatives of its light or place that state a need when they come
/// after the lamp ("I need the lamp brighter."). "Lighter" is left out: it
/// is said of weight too ("My lamp is lighter than yours."), and a bare
/// "need" or "want" is no need ("I want that lamp.", "We need a lamp like
/// that."). A comparative right before "than" compares and asks nothing:
/// "That lamp is brighter than mine." (Thirteen's P3 on #4239).
const MORE: &[&str] = &[
    "brighter", "dimmer", "darker", "warmer", "cooler", "higher", "lower", "closer",
];

/// Whether a clause naming the lamp asks it to move, or states a need of
/// its light or place: "too" before a light or place word ([`TOO_WHAT`]),
/// a comparative after the lamp and not before "than" ([`MORE`]), or a
/// word after the lamp that its light hurts ([`HURTS`]).
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
    // A move word first errs toward answering: "Light from that lamp is
    // nice." is answered too (Thirteen's P3 on #4219), the cheaper side.
    if words
        .peek()
        .is_some_and(|first| MOVES.contains(&first.as_str()))
    {
        return true;
    }
    let words: Vec<String> = words.collect();
    let too = words
        .windows(2)
        .any(|pair| TOO.contains(&pair[0].as_str()) && TOO_WHAT.contains(&pair[1].as_str()));
    let after_the_lamp = words
        .iter()
        .position(|word| {
            matches!(
                word.trim_end_matches("'s"),
                "lamp" | "lamps" | "lamb" | "lambs"
            )
        })
        .is_some_and(|at| {
            words[at..].iter().enumerate().any(|(i, word)| {
                HURTS.contains(&word.as_str())
                    || (MORE.contains(&word.as_str())
                        && words.get(at + i + 1).map(String::as_str) != Some("than"))
            })
        });
    too || after_the_lamp
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
            // Thirteen's P2 on #4219: "too", "want", "need" and "lighter"
            // alone are no request.
            "That lamp is cute too.",
            "That lamp is way too cute.",
            "I want that lamp.",
            "We need a lamp like that.",
            "My lamp is lighter than yours.",
            // Thirteen's P3 on #4239: a comparison asks nothing.
            "That lamp is brighter than mine.",
            "Your lamp is lower than mine.",
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
            "Wake up the lamp.",
            "The lamp, off please.",
            "Is the lamp on?",
            // Its moves and a stated need (Reviewer-three's narrower P2 on
            // #4201).
            "Lower the lamp.",
            "Raise the lamp a bit.",
            "Point the lamp at my desk.",
            "Tilt the lamp up.",
            "The lamp is too bright.",
            "I need the lamp brighter.",
            // A complaint about its light (Reviewer-three's P3 on #4239).
            "The lamp is so bright.",
            "The lamp is really dim.",
            "The lamp is blinding me.",
            "This lamp hurts my eyes.",
            "Do you want confidence? Going downstairs.",
            "Yes, please. A latte. No sugar.",
            "",
        ] {
            assert!(!talked_about(heard), "{heard}");
        }
    }
}
