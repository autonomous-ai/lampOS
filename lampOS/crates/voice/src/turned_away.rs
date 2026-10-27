//! A line turned to another person ends the conversation for that line, on
//! the second route in any room (#3882), off unless `LAMP_TURNED_AWAY=on`.
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
//! - it calls someone by a role at its start ("Mom, ...", "Guys, ...",
//!   "Mẹ ơi, ...");
//! - it asks someone for an object in the words of a request between people
//!   ("pass me", "hand me", "lend me", "đưa giùm"): the lamp has no hands.
//!   Bare "grab the", "fetch the" and "pass the" are left out, since people
//!   ask an assistant that way too;
//! - every part of it offers something or tells of going somewhere ("Do you
//!   want coffee?", "I'm going downstairs"): an errand between people. "I'm
//!   going downstairs, turn the light off" carries a command and plays.
//!
//! The signs are in English and Vietnamese, the second route's two
//! languages.
//!
//! Not only in a busy room: on lamp-06c1 on 28 October, with the product's
//! switches, the room was judged busy in 1 round of 6, and desk talk was
//! let out as going on in the quiet room's window all the same.
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
pub const HELD: &str = "a conversation with the lamp is going on, but the line turns to someone else: a role called, an object asked for, or an errand";

/// Words that call a person by a role at a line's start, before a comma.
/// The lamp is never called by these: it is called by its name.
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
    // Vietnamese: a role, then "ơi", calls a person.
    "mẹ ơi",
    "má ơi",
    "bố ơi",
    "ba ơi",
    "anh ơi",
    "chị ơi",
    "em ơi",
    "bạn ơi",
    "các bạn ơi",
    "mọi người ơi",
];

/// Words that ask a person for an object, or to hand one over: each names
/// the person asked ("me") or borrowing, the shape of a request between
/// people. Bare "grab the", "fetch the", "pass the", "hand over" and "fetch
/// me" are left out: people ask an assistant that way too ("Fetch the
/// forecast", "Pass the message on to my wife").
const OBJECT_ASKS: &[&str] = &[
    "pass me",
    "hand me",
    "toss me",
    "throw me",
    "lend me",
    "can i borrow",
    "could i borrow",
    // Vietnamese: hand over, pass along, lend.
    "đưa giùm",
    "đưa giúp",
    "chuyền giùm",
    "cho mình mượn",
    "cho tôi mượn",
    "cho em mượn",
];

/// Words that offer something to a person, or tell of going somewhere. A
/// line holds for one only when every part of it is such an errand: "I'm
/// going downstairs, turn the light off" carries a command to the lamp and
/// plays.
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
    "be right back",
    "i'm getting coffee",
    "im getting coffee",
    "i'm grabbing coffee",
    "im grabbing coffee",
    // Vietnamese.
    "đi mua cà phê",
    "uống cà phê không",
    "mình xuống dưới",
    "tôi xuống dưới",
    "em xuống dưới",
    "anh xuống dưới",
    "chị xuống dưới",
    "mình ra ngoài một chút",
    "tôi ra ngoài một chút",
];

/// Whether `heard` shows it is turned to someone else: a role called at
/// its start, an object asked for, or an errand between people.
pub fn turns_to_another(heard: &str) -> bool {
    let lower = heard.to_lowercase().replace('’', "'");
    let says = |words: &str| {
        let words = spaced(words);
        move |phrase: &&str| words.contains(&format!(" {phrase} "))
    };
    calls_a_role(&lower) || OBJECT_ASKS.iter().any(says(&lower)) || all_errand(&lower)
}

/// Short parts that offer something only beside an errand, never alone as
/// a line: "I'm going downstairs, want something?". "I want something
/// warmer" said to the lamp is no such part, being longer.
const OFFER_PARTS: &[&str] = &["want something", "want anything", "anything", "something"];

/// Whether every part of the line (split at commas and stops) is an
/// errand between people, and there is at least one part.
fn all_errand(lower: &str) -> bool {
    let parts: Vec<String> = lower
        .split([',', '.', ';', '!', '?'])
        .map(spaced)
        .filter(|part| !part.trim().is_empty())
        .collect();
    let errand = |part: &String| {
        ERRANDS
            .iter()
            .any(|phrase| part.contains(&format!(" {phrase} ")))
    };
    let offer = |part: &String| {
        OFFER_PARTS
            .iter()
            .any(|phrase| part == &format!(" {phrase} "))
    };
    parts.iter().any(errand) && parts.iter().all(|part| errand(part) || offer(part))
}

/// The words, lower case, one space apart, with a space at each end.
fn spaced(lower: &str) -> String {
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
    format!(
        " {} ",
        words.split_whitespace().collect::<Vec<_>>().join(" ")
    )
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
            "Mom, where are my keys?",
            "Guys, the meeting moved to three.",
            "I'll be right back.",
            "Want anything from the kitchen?",
            "Can I borrow your stapler?",
            "I’m going downstairs, want something?",
            "Mom, pass the remote.",
            "Could you toss me the tape?",
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
            // Mid-sentence words that are no ask (#3895's reads).
            "On the other hand, the forecast said rain?",
            "Will the storm pass the coast?",
            "I think it will pass the test.",
            // Requests to the lamp in the words of an ask (#3895's reads).
            "Can you grab the weather for tomorrow?",
            "Fetch the forecast for the weekend.",
            "Fetch me the news.",
            "Pass the message on to my wife.",
            "Hand over the details of the order.",
            // An errand with a command to the lamp after it plays.
            "I'm going downstairs, turn the light off.",
            "Good night, I'll be right back.",
            "Mom is calling, what time is it?",
            "I want something warmer.",
            "Something calmer, please.",
        ] {
            assert!(!turns_to_another(line), "{line}");
        }
    }

    /// Vietnamese desk talk turned to another person shows it too.
    #[test]
    fn vietnamese_desk_talk_turned_to_another_person_shows_it() {
        for line in [
            "Mẹ ơi, chìa khóa con đâu rồi?",
            "Anh ơi, tối nay ăn gì?",
            "Các bạn ơi, họp dời sang ba giờ.",
            "Đưa giùm mình cái sạc.",
            "Cho mình mượn cái bấm kim.",
            "Mình đi mua cà phê, có ai uống cà phê không?",
            "Tôi xuống dưới một chút.",
        ] {
            assert!(turns_to_another(line), "{line}");
        }
    }

    /// A Vietnamese follow-up or request to the lamp shows none of the signs.
    #[test]
    fn vietnamese_follow_ups_to_the_lamp_show_none() {
        for line in [
            "Thế còn ngày mai thì sao?",
            "Bật đèn sáng hơn.",
            "Mấy giờ rồi?",
            "Kể chuyện con rồng đi.",
            "Giúp tôi đặt hẹn giờ năm phút.",
            "Lấy giúp tôi tin tức hôm nay.",
            "Tôi xuống dưới, tắt đèn giúp tôi.",
            "Cảm ơn nhé.",
            "Mai mình đi sớm nhé",
        ] {
            assert!(!turns_to_another(line), "{line}");
        }
    }
}
