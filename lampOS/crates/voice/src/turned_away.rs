//! A line turned to another person ends the conversation for that line, on
//! the second route in a busy room (#3882), off unless `LAMP_TURNED_AWAY=on`.
//!
//! On lamp-4ace on 27 October, on stable 7632, in the busy scenes, after a
//! conversation with the lamp, two people talked to each other and the lamp
//! answered them: "Can you pass me the charger? The white one.", "Do you
//! want coffee? I'm going downstairs." Each was let out only as "a
//! conversation with the lamp is going on": the busy room's window lets out
//! any line within 15 s of the lamp's words, whoever says it. Desk talk held
//! swung from 5/6 to 1/6 on the same build, as the scene's lines fell inside
//! or outside that window behind answers of different lengths.
//!
//! With the switch, such a line is held when it shows it is turned to
//! someone else ([`turns_to_another`]):
//! - it calls someone by a role at its start ("Mom, ...", "Guys, ...");
//! - it asks someone for an object, or to hand one over ("pass me", "hand
//!   me", "grab the"): the lamp has no hands;
//! - it offers something or tells of going somewhere ("Do you want
//!   coffee?", "I'm going downstairs"): an errand between people.
//!
//! Only a line let out as the conversation going on alone is held: no name,
//! no call, nobody turned to the lamp, not said close to it, and not an
//! answer to the lamp's own question. A real follow-up ("And what about
//! tomorrow?") shows none of these signs, so it still plays. When unsure,
//! the line plays as before: the signs are words a person says to a person.

/// Where the coordinator reads the switch.
pub const ENVIRONMENT: &str = "LAMP_TURNED_AWAY";
/// The provider worker's flag for it, right after `--going-on-same-one`.
pub const FLAG: &str = "--turned-away";

/// Why the rule holds such a line, as its journal says it after "held
/// back: ".
pub const HELD: &str = "a busy room: a conversation with the lamp is going on, but the line turns to someone else: a role called, an object asked for, or an errand";

/// Words that call a person by a role at a line's start, before a comma.
const ROLES: &[&str] = &[
    "mom",
    "mum",
    "mommy",
    "dad",
    "daddy",
    "honey",
    "babe",
    "baby",
    "darling",
    "sweetie",
    "guys",
    "everyone",
    "everybody",
    "team",
    "folks",
    "boss",
    "buddy",
    "dude",
    "bro",
    "sis",
    "mate",
    "sir",
    "madam",
];

/// Words that ask a person for an object, or to hand one over.
const OBJECT_ASKS: &[&str] = &[
    "pass me",
    "pass the",
    "pass that",
    "pass it",
    "hand me",
    "hand over",
    "hand the",
    "hand that",
    "toss me",
    "throw me",
    "grab me",
    "grab the",
    "grab that",
    "fetch me",
    "fetch the",
    "lend me",
    "can i borrow",
    "could i borrow",
];

/// Words that offer something to a person, or tell of going somewhere.
const ERRANDS: &[&str] = &[
    "do you want coffee",
    "do you want a coffee",
    "do you want tea",
    "do you want a tea",
    "do you want lunch",
    "do you want anything",
    "do you want something",
    "want anything from",
    "want something from",
    "want some coffee",
    "want some tea",
    "i'm going downstairs",
    "im going downstairs",
    "i am going downstairs",
    "i'm going upstairs",
    "im going upstairs",
    "i'm going to the kitchen",
    "im going to the kitchen",
    "i'm heading out",
    "im heading out",
    "i'll be right back",
    "ill be right back",
    "be right back",
    "i'm getting coffee",
    "im getting coffee",
    "i'm grabbing coffee",
    "im grabbing coffee",
];

/// Whether `heard` shows it is turned to someone else: a role called at
/// its start, an object asked for, or an errand between people.
pub fn turns_to_another(heard: &str) -> bool {
    let lower = heard.to_lowercase().replace('’', "'");
    let words: String = lower
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '\'' {
                c
            } else {
                ' '
            }
        })
        .collect();
    let words = format!(
        " {} ",
        words.split_whitespace().collect::<Vec<_>>().join(" ")
    );
    let says = |phrase: &&str| words.contains(&format!(" {phrase} "));
    calls_a_role(&lower) || OBJECT_ASKS.iter().any(says) || ERRANDS.iter().any(says)
}

/// Whether the line begins by calling a role, as "Mom, ..." does: the
/// role, then a comma.
fn calls_a_role(lower: &str) -> bool {
    let start = lower.trim_start();
    let Some((first, _)) = start.split_once(',') else {
        return false;
    };
    ROLES.contains(&first.trim())
}

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
        let flags = ["x".to_owned(), FLAG.to_owned()];
        assert_eq!(take(&flags), (false, &flags[..]));
    }

    /// #3882's lines, and lines like them, are turned to someone else.
    #[test]
    fn desk_talk_turned_to_another_person_shows_it() {
        for line in [
            "Can you pass me the charger? The white one.",
            "Do you want coffee? I'm going downstairs.",
            "Hand me that pen.",
            "Could you grab the tape from the drawer?",
            "Mom, where are my keys?",
            "Guys, the meeting moved to three.",
            "I'll be right back.",
            "Want anything from the kitchen?",
            "Can I borrow your stapler?",
            "I’m going downstairs, want something?",
        ] {
            assert!(turns_to_another(line), "{line}");
        }
    }

    /// What is said to the lamp shows none of the signs, so it still plays.
    #[test]
    fn follow_ups_and_requests_to_the_lamp_show_none() {
        for line in [
            "And what about tomorrow?",
            "and what about tomorrow",
            "What's the weather tomorrow?",
            "Play it again.",
            "Turn the light warmer.",
            "Set a timer for five minutes.",
            "Thanks, that's perfect.",
            "No, the fox one.",
            "Yes, go ahead.",
            "Can you make it brighter?",
            "Give me the news.",
            "What time is it?",
            "Tell me a story about a dragon.",
            "Okay, stop.",
            "Mai mình đi sớm nhé",
            "Man, that was a long day.",
        ] {
            assert!(!turns_to_another(line), "{line}");
        }
    }
}
